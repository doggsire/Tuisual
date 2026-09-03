pragma Singleton
import QtQuick

// Central place to tweak the launcher's look: sizes, spacing, corner rounding and colors.
QtObject {
    // Text
    readonly property string fontFamily: "JetBrains Mono"
    readonly property int fontSize: 12

    // Shape
    readonly property int cornerRadius: 0
    readonly property int borderWidth: 1

    // Spacing
    readonly property int outerMargin: 5
    readonly property int panelSpacing: 5
    readonly property int panelPadding: 5
    readonly property int itemPadding: 4

    // Base palette (monochrome)
    readonly property color colorSurface: "#171717"
    readonly property real overlayOpacity: 0.30
    readonly property string colorBorder: "#a3a3a3"
    readonly property string colorPanel: "#262626"
    readonly property string colorHighlight: "#404040"
    readonly property string colorTextPrimary: "#f5f5f5"
    readonly property string colorTextSecondary: "#a3a3a3"
    readonly property string colorTextTertiary: "#d4d4d4"
    readonly property string colorTextMuted: "#737373"

    // Semantic accents (the only non-monochrome colors)
    readonly property string colorWarning: "#ffa500"
    readonly property string colorSuccess: "#16a34a"
    readonly property string colorError: "#dc2626"
}
