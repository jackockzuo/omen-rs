--!nonstrict
-- [[widget]] omen — bar widget: fan RPM + thermal mode + omend indicator + the
-- four WMAA temperatures. Thin presentation client over the `status` state
-- channel published by service.luau.
--
-- Left click  : open the control panel
-- Right click : toggle battery care
-- Middle click: host built-in "open this widget's settings"

local showLabel = noctalia.getConfig("show_label") ~= false
local showTemps = noctalia.getConfig("show_temps") ~= false

local status = {
    available = false,
    omend = false,
    sensors = {},
    fanRpm = {},
    power = {},
    thermal = "unknown",
    battery = nil,
}

local GLYPH = "car-fan"

local function fanText()
    local rpm = status.fanRpm or {}
    if #rpm == 0 then return nil end
    local parts = {}
    for _, r in ipairs(rpm) do
        table.insert(parts, tostring(r) .. " RPM")
    end
    return table.concat(parts, " / ")
end

local function thermalInfo()
    local m = status.thermal or "unknown"
    if m == "performance" then return "PERF", "primary" end
    if m == "balanced" then return "BAL", "secondary" end
    if m == "cool" then return "COOL", "tertiary" end
    if m == "quiet" then return "QUIET", "secondary" end
    if m == "extreme" then return "EXTREME", "primary" end
    return "?", "on_surface_variant"
end

local function tempText()
    local s = status.sensors or {}
    local parts = {}
    if type(s.ambient) == "number" then
        table.insert(parts, "Amb " .. tostring(s.ambient) .. "°")
    end
    if type(s.ir) == "number" then
        table.insert(parts, "IR " .. tostring(s.ir) .. "°")
    end
    return parts
end

local function tooltipRows()
    local rows = {}

    rows[#rows + 1] = {
        key = noctalia.tr("widget.omend"),
        value = status.omend and noctalia.tr("widget.omend-active")
            or noctalia.tr("widget.omend-down"),
    }

    local fan = fanText()
    if fan then
        rows[#rows + 1] = { key = noctalia.tr("widget.fan"), value = fan }
    end

    local s = status.sensors or {}
    if type(s.ambient) == "number" then
        rows[#rows + 1] = { key = noctalia.tr("widget.ambient"), value = tostring(s.ambient) .. " °C" }
    end
    if type(s.ir) == "number" then
        rows[#rows + 1] = { key = "IR", value = tostring(s.ir) .. " °C" }
    end
    if type(s.pch) == "number" then
        rows[#rows + 1] = { key = "PCH", value = tostring(s.pch) .. " °C" }
    end
    if type(s.vr) == "number" then
        rows[#rows + 1] = { key = "VR", value = tostring(s.vr) .. " °C" }
    end

    rows[#rows + 1] = {
        key = noctalia.tr("widget.thermal"),
        value = status.thermal or "unknown",
    }

    local p = status.power or {}
    if type(p.profile) == "string" then
        local v = p.profile
        if not p.consistent then v = v .. " ⚠" end
        rows[#rows + 1] = { key = noctalia.tr("widget.power"), value = v }
    end
    if type(p.platform_profile) == "string" then
        rows[#rows + 1] = { key = "platform_profile", value = p.platform_profile }
    end

    if type(status.battery) == "boolean" then
        rows[#rows + 1] = {
            key = noctalia.tr("widget.battery"),
            value = status.battery and noctalia.tr("widget.on") or noctalia.tr("widget.off"),
        }
    end

    rows[#rows + 1] = {
        key = noctalia.tr("widget.hint"),
        value = noctalia.tr("widget.hint-value"),
    }
    return rows
end

local function render()
    local nodes = {}

    nodes[#nodes + 1] = ui.glyph({ name = GLYPH, size = 14 })

    if not status.available then
        if showLabel then
            nodes[#nodes + 1] = ui.label({
                text = noctalia.tr("widget.unavailable"),
                color = "on_surface_variant",
            })
        end
    else
        local fan = fanText()
        if showLabel and fan then
            nodes[#nodes + 1] = ui.label({ text = fan, fontWeight = "medium" })
        end

        local tlabel, tcolor = thermalInfo()
        if showLabel then
            nodes[#nodes + 1] = ui.label({ text = tlabel, color = tcolor })
        end

        if showTemps then
            local temps = tempText()
            if #temps > 0 then
                nodes[#nodes + 1] = ui.label({
                    text = table.concat(temps, " "),
                    color = "on_surface_variant",
                })
            end
        end
    end

    -- omend 运行状态标志。
    if showLabel then
        nodes[#nodes + 1] = ui.glyph({
            name = status.omend and "circle-check-filled" or "circle-x",
            color = status.omend and "primary" or "error",
            size = 14,
        })
    end

    if barWidget.isVertical() then
        barWidget.render(ui.column({ gap = 2, align = "center" }, nodes))
    else
        barWidget.render(ui.row({ gap = 6, align = "center" }, nodes))
    end

    barWidget.setTooltip(tooltipRows())
end

local function applyStatus(value)
    if type(value) == "table" then
        status = value
    end
    render()
end

applyStatus(noctalia.state.get("status"))
noctalia.state.watch("status", applyStatus)

render()

function update()
    render()
end

function onClick()
    noctalia.togglePanel("jackockzuo/omen:panel")
end

function onRightClick()
    noctalia.state.set("command", "battery-toggle")
end

function onIpc(event, _payload)
    if event == "refresh" then
        noctalia.state.set("command", "refresh")
    elseif event == "settings" then
        noctalia.openSettings()
    else
        render()
    end
end
