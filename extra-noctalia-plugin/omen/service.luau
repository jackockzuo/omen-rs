--!nonstrict
-- HP OMEN BIOS/EC control — headless service.
--
-- Owns every `omen` shell call and polls one aggregated omend status snapshot,
-- normalizes it and publishes it on the plugin state channel.
-- The bar widget is a thin presentation client that reads that channel and
-- drives this service by writing to the `command` channel.
--
-- Design notes:
--   * Reads run through `omen status --json`; when the user is not root the CLI
--     transparently routes them through the omend socket (/tmp/omend.sock), so
--     no sudo is needed as long as omend is running.
--   * Writes touch EC/ACPI and therefore need root: they run as
--     `<write_cmd> <omen_bin> …` (default `pkexec`).
--   * No local persistence of state: after every write we re-poll and display
--     whatever the hardware reports (latest values only).

local SOCKET_PATH = "/tmp/omend.sock"
-- omend 的风扇模式控制文件：`auto`、`curve` 或 `manual:<0-100>`。
-- 风扇由 omend 统一执行，插件只切换模式。
local FAN_MODE_PATH = "/run/omend-fan-mode"
local DEFAULT_OMEN = "omen"

local omenBin = DEFAULT_OMEN
local resolvedOmenBin: string? = nil
local writeCmd = "pkexec"

local pending = false
local lastKey: string? = nil
local requestedThermal: string? = nil

-- Latest observed values, used only to compute the next cycle/toggle target.
local currentThermal = "unknown"
local lastBattery: boolean? = nil

-- 前向声明：writeFanMode 等辅助函数会调用 refresh。
local refresh

local function config(key, fallback)
    local value = noctalia.getConfig(key)
    if value == nil then
        return fallback
    end
    return value
end

local function shellQuote(s: string): string
    return "'" .. s:gsub("'", "'\\''") .. "'"
end

local function omenCmd(args: string): string
    -- 写命令走 pkexec 时 PATH 会被重置，所以统一用解析出的绝对路径。
    local bin = resolvedOmenBin or omenBin
    return shellQuote(bin) .. " " .. args
end

local function logX(s: string)
    noctalia.log("[OMEN] " .. s)
end

local function isOmendActive(): boolean
    return noctalia.fileExists(SOCKET_PATH) == true
end

-- 写风扇模式。omend 每轮读取并按此执行。
local function writeFanMode(content)
    local ok, err = noctalia.writeFile(FAN_MODE_PATH, content .. "\n")
    if not ok then
        logX("write fan-mode failed: " .. tostring(err))
        noctalia.notifyError(noctalia.tr("notify.title"), noctalia.tr("notify.fan-mode-failed"))
    end
    refresh()
end

-- ---------------------------------------------------------------------------
-- JSON decoding (omen … --json emits pretty-printed serde_json).
-- ---------------------------------------------------------------------------

local function decodeJson(stdout: string)
    if type(stdout) ~= "string" or stdout == "" then
        return nil
    end
    local ok, decoded = pcall(function()
        return noctalia.json.decode(stdout)
    end)
    if not ok or type(decoded) ~= "table" then
        return nil
    end
    return decoded
end

-- ---------------------------------------------------------------------------
-- Polling + publish
-- ---------------------------------------------------------------------------

local function statusKey(status)
    local s = status.sensors or {}
    local rpm = status.fanRpm or {}
    local p = status.power or {}
    return table.concat({
        tostring(status.available),
        tostring(status.omend),
        tostring(s.ir), tostring(s.ambient), tostring(s.pch), tostring(s.vr),
        table.concat(rpm, ","),
        tostring(status.fanMode),
        tostring(status.fanSpeed),
        tostring(p.profile),
        tostring(p.consistent),
        tostring(p.platform_profile),
        tostring(status.thermal),
        tostring(status.battery),
        tostring(status.updatedAtMs),
        tostring(status.requestedThermal),
        table.concat(status.errors or {}, ","),
    }, "|")
end

local function publish(result)
    local raw = type(result) == "table" and result.exitCode == 0 and decodeJson(result.stdout) or nil
    local status = {
        available = raw ~= nil and raw.available == true,
        omend = isOmendActive(),
        sensors = raw and raw.sensors or {},
        cpuTemp = raw and raw.cpuTemp or nil,
        fanRpm = {},
        fanMode = raw and raw.fanMode or "auto",
        fanSpeed = raw and raw.fanSpeed or nil,
        fanCurveConfigured = raw and raw.fanCurveConfigured == true or false,
        fanCurve = raw and raw.fanCurve or {},
        power = raw and raw.power or {},
        thermal = "unknown",
        thermalObserved = "unknown",
        battery = nil,
        errors = raw and raw.errors or { "omend status unavailable" },
        updatedAtMs = raw and raw.updatedAtMs or nil,
        requestedThermal = requestedThermal,
    }

    local fan = raw and raw.fan
    if type(fan) == "table" and type(fan.fans) == "table" then
        for _, f in ipairs(fan.fans) do
            if type(f) == "table" and type(f.level) == "number" then
                table.insert(status.fanRpm, f.level * 100)
            end
        end
    end

    local t = raw and raw.thermal
    if type(t) == "table" and type(t.mode) == "string" then
        status.thermalObserved = t.mode
        status.thermal = t.mode
    end
    local p = status.power
    if status.thermal == "unknown" and type(p.profile) == "string"
        and (p.profile == "performance" or p.profile == "balanced") then
        status.thermal = p.profile
    end
    local b = raw and raw.battery
    if type(b) == "table" and type(b.care) == "boolean" then
        status.battery = b.care
    end

    currentThermal = status.thermal
    lastBattery = status.battery

    local key = statusKey(status)
    if key ~= lastKey then
        lastKey = key
        noctalia.state.set("status", status)
    end
end

function refresh()
    if pending then return end
    pending = true

    local started = noctalia.runAsync(omenCmd("status --json"), function(result)
        pending = false
        publish(result)
    end)

    if not started then
        pending = false
        publish({ exitCode = -1, stdout = "" })
    end
end

-- ---------------------------------------------------------------------------
-- Write actions (root via pkexec/sudo)
-- ---------------------------------------------------------------------------

local THERMAL_CYCLE = { "performance", "balanced", "cool", "quiet", "extreme" }

local function nextThermal(current)
    for i, m in ipairs(THERMAL_CYCLE) do
        if m == current then
            return THERMAL_CYCLE[(i % #THERMAL_CYCLE) + 1]
        end
    end
    return THERMAL_CYCLE[1]
end

local function runWrite(args)
    local cmd = writeCmd .. " " .. omenCmd(args)
    local started = noctalia.runAsync(cmd, function(result)
        if result.exitCode ~= 0 then
            noctalia.notifyError(
                noctalia.tr("notify.title"),
                noctalia.tr("notify.write-failed", { cmd = args })
            )
            logX("write failed: " .. args)
        end
        refresh()
    end)

    if not started then
        noctalia.notifyError(noctalia.tr("notify.title"), noctalia.tr("notify.write-spawn-failed"))
        refresh()
    end
end

local function handleCommand(command)
    local action = command
    local mode, speed
    if type(command) == "table" then
        action = command.action
        mode = command.mode
        speed = command.speed
    end

    if action == "cycle-thermal" then
        requestedThermal = nextThermal(requestedThermal or currentThermal)
        runWrite("thermal " .. requestedThermal)
    elseif action == "thermal-set" then
        if type(mode) == "string" and ({ performance = true, balanced = true, cool = true, quiet = true, extreme = true })[mode] then
            requestedThermal = mode
            runWrite("thermal " .. mode)
        end
    elseif action == "fan-set" then
        if type(speed) == "number" and speed >= 0 and speed <= 100 then
            writeFanMode("manual:" .. math.floor(speed))
        end
    elseif action == "fan-curve" then
        writeFanMode("curve")
    elseif action == "fan-auto" then
        writeFanMode("auto")
    elseif action == "battery-toggle" then
        runWrite("battery " .. (lastBattery ~= true and "on" or "off"))
    elseif action == "battery-set" then
        runWrite("battery " .. (command.enabled == true and "on" or "off"))
    elseif action == "power-set" then
        if mode == "balanced" or mode == "performance" then
            runWrite("power profile set " .. mode)
        end
    elseif action == "refresh" then
        refresh()
    else
        logX("unknown command: " .. tostring(action))
    end
end

-- ---------------------------------------------------------------------------
-- Lifecycle
-- ---------------------------------------------------------------------------

local function isAbsolutePath(p)
    return type(p) == "string" and p:sub(1, 1) == "/"
end

-- pkexec / sudo 都会重置 PATH，因此写命令需要 omen 的绝对路径。
-- 这里把 `omen_bin`（可能是裸命令名）解析成绝对路径，`command -v` 拿不到就回退原值。
local function resolveOmenBin()
    if isAbsolutePath(omenBin) then
        resolvedOmenBin = omenBin
        return
    end

    local started = noctalia.runAsync("command -v " .. shellQuote(omenBin), function(result)
        if result.exitCode == 0 and type(result.stdout) == "string" then
            local path = result.stdout:gsub("^%s+", ""):gsub("%s+$", "")
            if isAbsolutePath(path) then
                resolvedOmenBin = path
                logX("resolved omen -> " .. path)
                return
            end
        end
        resolvedOmenBin = omenBin
        logX("failed to resolve omen, using: " .. omenBin)
    end)

    if not started then
        resolvedOmenBin = omenBin
    end
end

local function init()
    omenBin = config("omen_bin", DEFAULT_OMEN)
    if type(omenBin) ~= "string" or omenBin == "" then
        omenBin = DEFAULT_OMEN
    end

    writeCmd = config("write_cmd", "pkexec")
    if writeCmd == "pkexec" and not noctalia.commandExists("pkexec") then
        noctalia.notifyError(noctalia.tr("notify.title"), noctalia.tr("notify.no-pkexec"))
    end

    resolveOmenBin()
end

init()

local pollInterval = tonumber(config("poll_interval", 2000)) or 2000
noctalia.setUpdateInterval(pollInterval)

noctalia.state.watch("command", handleCommand)

refresh()

function update()
    refresh()
end
