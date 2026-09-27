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
    power = {},
    thermal = "unknown",
    battery = nil,
}

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

local function rpmToPercent(rpm)
    return math.max(0, math.min(100, math.floor((rpm / 6400) * 100 + 0.5)))
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
    local thermalButtons = {}
    for _, m in ipairs(THERMAL_MODES) do
        table.insert(thermalButtons, ui.button({
            text = noctalia.tr(THERMAL_LABELS[m]),
            tooltip = m,
            variant = status.thermal == m and "primary" or "outline",
            controlSize = "sm",
            flexGrow = 1,
            onClick = function()
                noctalia.state.set("command", { action = "thermal-set", mode = m })
            end,
        }))
    end
    table.insert(children, ui.column({ gap = 6 }, {
        sectionTitle(noctalia.tr("panel.thermal-set")),
        ui.row({ gap = 4, align = "stretch" }, thermalButtons),
    }))
    table.insert(children, ui.separator({ thickness = 1, color = "outline/0.3", spacing = 8 }))

    -- 风扇滑杆（0-100% = omen fan set 的百分比参数）+ 应用/恢复自动
    table.insert(children, ui.column({ gap = 6 }, {
        ui.row({ align = "center", justify = "space_between" }, {
            sectionTitle(noctalia.tr("panel.fan-set")),
            ui.label({ text = tostring(fanSlider or 0) .. "%", fontSize = 14, fontWeight = "bold", color = "primary" }),
        }),
        ui.row({ align = "center", gap = 8 }, {
            ui.label({ text = "0%", fontSize = 11, color = "on_surface_variant" }),
            ui.slider({
                min = 0,
                max = 100,
                step = 5,
                value = fanSlider or 0,
                flexGrow = 1,
                onChange = "onFanSlider",
            }),
            ui.label({ text = "100%", fontSize = 11, color = "on_surface_variant" }),
        }),
        ui.row({ gap = 8 }, {
            ui.button({ text = noctalia.tr("panel.fan-apply"), variant = "primary", flexGrow = 1, onClick = "onFanApply" }),
            ui.button({ text = noctalia.tr("panel.fan-auto"), variant = "ghost", flexGrow = 1, onClick = "onFanAuto" }),
        }),
    }))
    table.insert(children, ui.separator({ thickness = 1, color = "outline/0.3", spacing = 8 }))

    -- 电池养护
    table.insert(children, ui.button({
        text = noctalia.tr("panel.battery-toggle"),
        variant = "outline",
        onClick = "onBatteryToggle",
    }))

    panel.render(ui.column({ gap = 9, padding = 14, flexGrow = 1 }, children))
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
    noctalia.state.set("command", { action = "fan-set", speed = fanSlider or 0 })
end

function onFanAuto()
    noctalia.state.set("command", { action = "fan-auto" })
end

function onBatteryToggle()
    noctalia.state.set("command", { action = "battery-toggle" })
end

local function applyStatus(value)
    if type(value) == "table" then
        status = value
        if fanSlider == nil then
            local rpm = status.fanRpm or {}
            if #rpm > 0 then
                fanSlider = rpmToPercent(rpm[1])
            else
                fanSlider = 50
            end
        end
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
