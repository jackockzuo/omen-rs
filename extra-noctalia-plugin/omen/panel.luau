--!nonstrict
-- [[panel]] panel — OMEN 控制面板：查看信息 + 调整风扇/热模式/电池/解锁。
--
-- 打开方式：左键点击小组件，或
--   noctalia msg panel-toggle jackockzuo/omen:panel
--
-- 面板是薄客户端：从 `status` 状态通道读数据，写操作通过 `command` 通道
-- 交给 service.luau（写命令需要 root，走 pkexec/sudo）。

local isOpen = false
local fanSlider: number? = nil

local status = {
    available = false,
    omend = false,
    sensors = {},
    fanRpm = {},
    fanMode = "auto",
    power = {},
    thermal = "unknown",
    battery = nil,
}
local lastFanMode: string? = nil
local lastFanSpeed: number? = nil

local THERMAL_MODES = { "performance", "balanced", "cool", "quiet", "extreme" }

local THERMAL_LABELS = {
    performance = "panel.mode.performance",
    balanced = "panel.mode.balanced",
    cool = "panel.mode.cool",
    quiet = "panel.mode.quiet",
    extreme = "panel.mode.extreme",
}

local POWER_PROFILES = { "balanced", "performance" }

local POWER_LABELS = {
    balanced = "panel.power.balanced",
    performance = "panel.power.performance",
}

local function thermalSelection()
    local selected = status.requestedThermal or status.thermalObserved or status.thermal
    for _, mode in ipairs(THERMAL_MODES) do
        if mode == selected then return mode end
    end
    return nil
end

local function fanRpmText()
    local rpm = status.fanRpm or {}
    if #rpm == 0 then return nil end
    local parts = {}
    for _, r in ipairs(rpm) do
        table.insert(parts, tostring(r) .. " RPM")
    end
    return table.concat(parts, " / ")
end

local function temp(s)
    return type(s) == "number" and (tostring(s) .. " °C") or "--"
end

local function tempsText()
    local s = status.sensors or {}
    local parts = {}
    if type(s.ambient) == "number" then table.insert(parts, "Amb " .. tostring(s.ambient) .. "°") end
    if type(s.ir) == "number" then table.insert(parts, "IR " .. tostring(s.ir) .. "°") end
    if type(s.pch) == "number" then table.insert(parts, "PCH " .. tostring(s.pch) .. "°") end
    if type(s.vr) == "number" then table.insert(parts, "VR " .. tostring(s.vr) .. "°") end
    if #parts == 0 then return "--" end
    return table.concat(parts, "  ")
end

local function boolText(value, yesKey, noKey)
    if type(value) ~= "boolean" then return "--" end
    return value and noctalia.tr(yesKey) or noctalia.tr(noKey)
end

local function powerText()
    local p = status.power or {}
    if type(p.profile) ~= "string" then return "--" end
    local t = p.profile
    if p.consistent == false then t = t .. " ⚠" end
    return t
end

local function powerColor()
    local p = status.power or {}
    if p.consistent == false then return "error" end
    return nil
end

-- label/value 信息行：固定宽度标签 + 值。
local function infoRow(label, value, color)
    return ui.row({ gap = 6, align = "start" }, {
        ui.column({ width = 88 }, {
            ui.label({ text = label, fontSize = 12, color = "on_surface_variant" }),
        }),
        ui.label({
            text = value,
            fontSize = 12,
            color = color or "on_surface",
            flexGrow = 1,
            maxLines = 1,
        }),
    })
end

local function sectionTitle(text)
    return ui.label({ text = text, fontSize = 12, fontWeight = "bold", color = "on_surface_variant" })
end

local function renderPanel()
    if not isOpen then return end

    local p = status.power or {}

    local omendColor = status.omend and "primary" or "error"
    local omendText = status.omend and noctalia.tr("panel.omend-active") or noctalia.tr("panel.omend-down")

    local children = {
        ui.row({ align = "center", justify = "space_between" }, {
            ui.column({ gap = 2, flexGrow = 1 }, {
                ui.label({ text = noctalia.tr("panel.title"), fontSize = 16, fontWeight = "bold", color = "primary" }),
                ui.label({ text = noctalia.tr("panel.subtitle"), fontSize = 12, color = "on_surface_variant" }),
            }),
            ui.button({ glyph = "close", variant = "ghost", onClick = function()
                panel.close()
            end }),
        }),
        ui.separator({ thickness = 1, color = "outline/0.3", spacing = 8 }),
    }

    -- 信息区（紧凑）
    table.insert(children, ui.column({ gap = 5 }, {
        infoRow(noctalia.tr("panel.omend"), omendText, omendColor),
        infoRow(noctalia.tr("panel.fan"), fanRpmText() or "--"),
        infoRow(noctalia.tr("panel.cpu-temp"), temp(status.cpuTemp)),
        infoRow(noctalia.tr("panel.temps"), tempsText()),
        infoRow(noctalia.tr("panel.thermal"), status.thermal or "unknown"),
        infoRow(noctalia.tr("panel.power-profile"), powerText(), powerColor()),
        infoRow(noctalia.tr("panel.battery"), boolText(status.battery, "panel.on", "panel.off")),
    }))
    table.insert(children, ui.separator({ thickness = 1, color = "outline/0.3", spacing = 8 }))

    -- 功耗配置（均衡 / 性能解锁）
    local powerButtons = {}
    for _, m in ipairs(POWER_PROFILES) do
        table.insert(powerButtons, ui.button({
            text = noctalia.tr(POWER_LABELS[m]),
            tooltip = m,
            variant = p.profile == m and "primary" or "outline",
            controlSize = "sm",
            flexGrow = 1,
            onClick = function()
                noctalia.state.set("command", { action = "power-set", mode = m })
            end,
        }))
    end
    table.insert(children, ui.column({ gap = 6 }, {
        sectionTitle(noctalia.tr("panel.power-set")),
        ui.row({ gap = 4, align = "stretch" }, powerButtons),
    }))
    table.insert(children, ui.separator({ thickness = 1, color = "outline/0.3", spacing = 8 }))

    -- 热模式按钮（短标签 + 完整名 tooltip）
    local thermalRows = { {}, {} }
    local selectedThermal = thermalSelection()
    for i, m in ipairs(THERMAL_MODES) do
        local button = ui.button({
            text = noctalia.tr(THERMAL_LABELS[m]),
            tooltip = m,
            variant = selectedThermal == m and "primary" or "outline",
            controlSize = "sm",
            flexGrow = 1,
            onClick = function()
                noctalia.state.set("command", { action = "thermal-set", mode = m })
            end,
        })
        table.insert(thermalRows[i <= 3 and 1 or 2], button)
    end
    table.insert(children, ui.column({ gap = 6 }, {
        sectionTitle(noctalia.tr("panel.thermal-set")),
        ui.row({ gap = 4, align = "stretch" }, thermalRows[1]),
        ui.row({ gap = 4, align = "stretch" }, thermalRows[2]),
        ui.label({
            text = noctalia.tr("panel.thermal-observed") .. ": "
                .. tostring(status.thermalObserved or status.thermal or "unknown")
                .. (status.requestedThermal and (" · " .. noctalia.tr("panel.thermal-requested") .. ": " .. status.requestedThermal) or ""),
            fontSize = 11,
            color = "on_surface_variant",
            maxLines = 2,
        }),
    }))
    table.insert(children, ui.separator({ thickness = 1, color = "outline/0.3", spacing = 8 }))

    -- 风扇：温度曲线（默认）/ 手动。曲线由 omend 执行，不用 BIOS 自动。
    local fanMode = status.fanMode or "auto"
    local fanModeText = fanMode == "manual" and (noctalia.tr("panel.fan-mode.manual") .. " " .. tostring(status.fanSpeed or "--") .. "%")
        or (fanMode == "curve" and noctalia.tr("panel.fan-mode.curve") or noctalia.tr("panel.fan-mode.auto"))
    local curvePoints = {}
    for _, point in ipairs(status.fanCurve or {}) do
        table.insert(curvePoints, tostring(point.temp) .. "°→" .. tostring(point.speed) .. "%")
    end
    table.insert(children, ui.column({ gap = 6 }, {
        ui.row({ align = "center", justify = "space_between" }, {
            sectionTitle(noctalia.tr("panel.fan-set")),
            ui.label({
                text = fanModeText,
                fontSize = 12,
                color = fanMode == "manual" and "primary" or "secondary",
            }),
        }),
        ui.row({ align = "center", gap = 8 }, {
            ui.label({ text = "0%", fontSize = 11, color = "on_surface_variant" }),
            ui.slider({
                min = 0,
                max = 100,
                step = 5,
                value = fanSlider or (fanMode == "manual" and status.fanSpeed or 50),
                flexGrow = 1,
                onChange = "onFanSlider",
            }),
            ui.label({ text = "100%", fontSize = 11, color = "on_surface_variant" }),
        }),
        ui.row({ gap = 8 }, {
            ui.button({
                text = noctalia.tr("panel.fan-auto"),
                variant = fanMode == "auto" and "primary" or "outline",
                flexGrow = 1,
                onClick = "onFanAuto",
            }),
            ui.button({
                text = noctalia.tr("panel.fan-curve"),
                variant = fanMode == "curve" and "primary" or "outline",
                enabled = status.fanCurveConfigured == true,
                flexGrow = 1,
                tooltip = status.fanCurveConfigured and table.concat(curvePoints, "  ") or noctalia.tr("panel.fan-no-curve"),
                onClick = "onFanCurve",
            }),
            ui.button({
                text = noctalia.tr("panel.fan-apply"),
                variant = "primary",
                flexGrow = 1,
                onClick = "onFanApply",
            }),
        }),
    }))
    table.insert(children, ui.separator({ thickness = 1, color = "outline/0.3", spacing = 8 }))

    -- 电池养护
    table.insert(children, ui.row({ align = "center", justify = "space_between" }, {
        ui.column({ gap = 2 }, {
            sectionTitle(noctalia.tr("panel.battery-toggle")),
            ui.label({ text = boolText(status.battery, "panel.on", "panel.off"), fontSize = 11, color = "on_surface_variant" }),
        }),
        ui.toggle({ checked = status.battery == true, enabled = type(status.battery) == "boolean", onChange = "onBatteryToggle" }),
    }))

    if type(status.errors) == "table" and #status.errors > 0 then
        table.insert(children, ui.label({
            text = noctalia.tr("panel.read-errors") .. ": " .. table.concat(status.errors, " · "),
            fontSize = 11,
            color = "error",
            maxLines = 3,
        }))
    end
    if type(status.updatedAtMs) == "number" then
        table.insert(children, ui.label({
            text = noctalia.tr("panel.updated") .. " " .. os.date("%H:%M:%S", math.floor(status.updatedAtMs / 1000)),
            fontSize = 10,
            color = "on_surface_variant",
        }))
    end

    local body = {}
    for i = 3, #children do
        table.insert(body, children[i])
    end
    panel.render(ui.column({ gap = 0, padding = 14, flexGrow = 1 }, {
        children[1],
        children[2],
        ui.scroll({ gap = 9, flexGrow = 1 }, body),
    }))
end

function onOpen(_context)
    isOpen = true
    -- 打开时立即取一次最新状态。
    noctalia.state.set("command", "refresh")
    renderPanel()
end

function onClose()
    isOpen = false
end

function onFanSlider(value: string)
    local num = tonumber(value)
    if num then
        fanSlider = math.floor(num + 0.5)
        renderPanel()
    end
end

function onFanApply()
    noctalia.state.set("command", { action = "fan-set", speed = fanSlider or status.fanSpeed or 50 })
end

function onFanCurve()
    noctalia.state.set("command", "fan-curve")
end

function onFanAuto()
    noctalia.state.set("command", "fan-auto")
end

function onBatteryToggle(value: string)
    noctalia.state.set("command", { action = "battery-set", enabled = value == "true" })
end

local function applyStatus(value)
    if type(value) == "table" then
        status = value
        if status.fanMode == "manual" and type(status.fanSpeed) == "number"
            and (lastFanMode ~= status.fanMode or lastFanSpeed ~= status.fanSpeed) then
            fanSlider = status.fanSpeed
        elseif fanSlider == nil then
            fanSlider = 50
        end
        lastFanMode = status.fanMode
        lastFanSpeed = status.fanSpeed
    end
    if isOpen then
        renderPanel()
    end
end

noctalia.state.watch("status", applyStatus)

local initial = noctalia.state.get("status")
if type(initial) == "table" then
    applyStatus(initial)
end

function update()
    if isOpen then
        renderPanel()
    end
end
