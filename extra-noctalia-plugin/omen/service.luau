--!nonstrict
-- HP OMEN BIOS/EC control — headless service.
--
-- Owns every `omen` shell call and runs for the whole session. It polls the
-- read-only commands (`sensors` / `fan` / `perf` / `thermal` / `battery`),
-- normalizes the JSON into one `status` table and publishes it on the plugin
-- state channel. The bar widget is a thin presentation client that reads that
-- channel and drives this service by writing to the `command` channel.
--
-- Design notes:
--   * Reads run through `omen <cmd> --json`; when the user is not root the CLI
--     transparently routes them through the omend socket (/tmp/omend.sock), so
--     no sudo is needed as long as omend is running.
--   * Writes touch EC/ACPI and therefore need root: they run as
--     `<write_cmd> <omen_bin> …` (default `pkexec`).
--   * No local persistence of state: after every write we re-poll and display
--     whatever the hardware reports (latest values only).

local SOCKET_PATH = "/tmp/omend.sock"
local DEFAULT_OMEN = "omen"

local omenBin = DEFAULT_OMEN
local resolvedOmenBin: string? = nil
local writeCmd = "pkexec"

local pending = false
local lastKey: string? = nil

-- Latest observed values, used only to compute the next cycle/toggle target.
local currentThermal = "unknown"
local lastBattery: boolean? = nil

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

local function parseSensors(stdout)
    local j = decodeJson(stdout)
    if not j then return nil end
    return {
        ir = j.ir,
        ambient = j.ambient,
        pch = j.pch,
        vr = j.vr,
    }
end

-- `omen fan --json` -> { fans = [ { level = N }, … ] }; N ∈ 0..64 maps to
-- 0..6400 RPM (each step is 100 RPM).
local function parseFan(stdout)
    local j = decodeJson(stdout)
    if not j or type(j.fans) ~= "table" then return nil end
    local rpm = {}
    for _, f in ipairs(j.fans) do
        if type(f) == "table" and type(f.level) == "number" then
            table.insert(rpm, f.level * 100)
        end
    end
    return rpm
end

-- `omen power profile --json` ->
-- { profile = "performance"|"balanced"|"inconsistent"|"unknown", consistent, … }.
local function parsePower(stdout)
    local j = decodeJson(stdout)
    if not j then return nil end
    local inconsistencies = {}
    if type(j.inconsistencies) == "table" then
        for _, item in ipairs(j.inconsistencies) do
            table.insert(inconsistencies, tostring(item))
        end
    end
    return {
        profile = j.profile or "unknown",
        consistent = j.consistent == true,
        ec_0xba = j.ec_0xba,
        ec_0x95 = j.ec_0x95,
        platform_profile = j.platform_profile,
        inconsistencies = inconsistencies,
    }
end

-- `omen thermal --json` -> { mode = "performance"|"balanced"|"unknown", … }.
-- Note: the 0x1A strategy register is write-only, so this is derived from
-- platform_profile / EC 0x95 (see omen-rs src/commands/thermal.rs).
local function parseThermal(stdout)
    local j = decodeJson(stdout)
    if not j then return nil end
    return j.mode or "unknown"
end

-- `omen battery --json` -> { care = bool }.
local function parseBattery(stdout)
    local j = decodeJson(stdout)
    if not j then return nil end
    return j.care == true
end

-- ---------------------------------------------------------------------------
-- Polling + publish
-- ---------------------------------------------------------------------------

local READ_QUEUE = {
    { key = "sensors", cmd = "sensors --json" },
    { key = "fan",     cmd = "fan --json" },
    { key = "power",   cmd = "power profile --json" },
    { key = "thermal", cmd = "thermal status --json" },
    { key = "battery", cmd = "battery status --json" },
}

local function statusKey(status)
    local s = status.sensors or {}
    local rpm = status.fanRpm or {}
    local p = status.power or {}
    return table.concat({
        tostring(status.available),
        tostring(status.omend),
        tostring(s.ir), tostring(s.ambient), tostring(s.pch), tostring(s.vr),
        table.concat(rpm, ","),
        tostring(p.profile),
        tostring(p.consistent),
        tostring(p.platform_profile),
        tostring(status.thermal),
        tostring(status.battery),
    }, "|")
end

local function publish(partial)
    local status = {
        available = false,
        omend = isOmendActive(),
        sensors = {},
        fanRpm = {},
        power = {},
        thermal = "unknown",
        battery = nil,
    }

    local function ok(result)
        return type(result) == "table" and result.exitCode == 0
    end

    local sr = partial.sensors
    if ok(sr) then
        local s = parseSensors(sr.stdout)
        if s then
            status.sensors = s
            status.available = true
        end
    end

    local fr = partial.fan
    if ok(fr) then
        local rpm = parseFan(fr.stdout)
        if rpm then
            status.fanRpm = rpm
            status.available = true
        end
    end

    local pr = partial.power
    if ok(pr) then
        local p = parsePower(pr.stdout)
        if p then
            status.power = p
            status.available = true
        end
    end

    local tr = partial.thermal
    if ok(tr) then
        local t = parseThermal(tr.stdout)
        if t then
            status.thermal = t
        end
    end

    -- 回退：旧版 omend 没有 `thermal status`，此时从 power.profile 推导。
    if status.thermal == "unknown" and type(status.power.profile) == "string" then
        local pp = status.power.profile
        if pp == "performance" or pp == "balanced" then
            status.thermal = pp
        end
    end

    local br = partial.battery
    if ok(br) then
        local b = parseBattery(br.stdout)
        if b ~= nil then
            status.battery = b
        end
    end

    currentThermal = status.thermal
    lastBattery = status.battery

    local key = statusKey(status)
    if key ~= lastKey then
        lastKey = key
        noctalia.state.set("status", status)
    end
end

local function refresh()
    if pending then return end
    pending = true

    local partial = {}

    local function step(i)
        local item = READ_QUEUE[i]
        if not item then
            pending = false
            publish(partial)
            return
        end

        local started = noctalia.runAsync(omenCmd(item.cmd), function(result)
            partial[item.key] = result
            step(i + 1)
        end)

        if not started then
            partial[item.key] = { exitCode = -1, stdout = "" }
            step(i + 1)
        end
    end

    step(1)
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
        runWrite("thermal " .. nextThermal(currentThermal))
    elseif action == "thermal-set" then
        if type(mode) == "string" then
            runWrite("thermal " .. mode)
        end
    elseif action == "fan-set" then
        if type(speed) == "number" and speed >= 0 and speed <= 100 then
            runWrite("fan set " .. math.floor(speed))
        end
    elseif action == "fan-auto" then
        runWrite("fan auto")
    elseif action == "battery-toggle" then
        runWrite("battery " .. (lastBattery ~= true and "on" or "off"))
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
