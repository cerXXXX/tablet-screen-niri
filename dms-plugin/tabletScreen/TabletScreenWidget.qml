import QtQuick
import Quickshell
import Quickshell.Io
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

// Turns the tablet's second screen on and off.
//
// The daemon owns the output: it creates the virtual output when a tablet is
// attached and this switch is on, and removes it otherwise, which is what
// makes "off" cost nothing at all — no compositor rendering, no capture, no
// encoder. Everything here is one command over that daemon's control socket
// (`uscreen display on|off|toggle|status`), so the pill, a keybinding and a
// script all drive the same switch.
PluginComponent {
    id: root

    property var popoutService: null

    // "on" / "off" / "absent" (no daemon, or no tablet plugged in)
    property string screenState: "absent"

    readonly property var presets: [
        {
            "label": "2400×1500 @ 60",
            "width": 2400,
            "height": 1500,
            "fps": 60
        },
        {
            "label": "3000×1872 @ 50",
            "width": 3000,
            "height": 1872,
            "fps": 50
        },
        {
            "label": "3000×1872 @ 60",
            "width": 3000,
            "height": 1872,
            "fps": 60
        }
    ]

    readonly property bool isOn: screenState === "on"

    // Poll: the switch can be changed from anywhere (a keybinding, a script),
    // and the daemon can change it by itself when the cable appears.
    Timer {
        interval: 3000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: statusProc.running = true
    }

    Process {
        id: statusProc
        command: ["sh", "-c", "uscreen display status 2>/dev/null"]
        stdout: StdioCollector {
            onStreamFinished: {
                const text = this.text.trim();
                root.screenState = (text === "on" || text === "off") ? text : "absent";
            }
        }
    }

    Process {
        id: toggleProc
        command: ["uscreen", "display", "toggle"]
        onExited: statusProc.running = true
    }

    Process {
        id: presetProc
        onExited: {
            root.screenState = "off";
            refreshTimer.running = true;
        }
    }

    Timer {
        id: refreshTimer
        interval: 4000
        onTriggered: statusProc.running = true
    }

    function applyPreset(preset) {
        const cfg = "$HOME/tablet-screen/tools/uscreen-config.py";
        presetProc.command = ["sh", "-c", cfg + " set auto_resolution false && " + cfg + " set width " + preset.width + " && " + cfg + " set height " + preset.height + " && " + cfg + " set fps " + preset.fps + " && systemctl --user restart uscreen"];
        presetProc.running = true;
    }

    function openSettings() {
        Quickshell.execDetached(["uscreen-gui"]);
    }

    horizontalBarPill: Component {
        StyledRect {
            width: row.implicitWidth + Theme.spacingM * 2
            height: parent.widgetThickness
            radius: Theme.cornerRadius
            color: root.isOn ? Theme.primary : Theme.surfaceContainerHigh

            Row {
                id: row
                anchors.centerIn: parent
                spacing: Theme.spacingS

                StyledText {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.isOn ? "tablet_mac" : "tablet_mac"
                    font.family: "Material Symbols Outlined"
                    font.pixelSize: Theme.fontSizeMedium
                    color: root.isOn ? Theme.onPrimary : Theme.surfaceText
                }

                StyledText {
                    anchors.verticalCenter: parent.verticalCenter
                    text: root.screenState === "absent" ? "—" : (root.isOn ? "on" : "off")
                    font.pixelSize: Theme.fontSizeSmall
                    color: root.isOn ? Theme.onPrimary : Theme.surfaceText
                }
            }
        }
    }

    verticalBarPill: Component {
        StyledRect {
            width: parent.widgetThickness
            height: row.implicitHeight + Theme.spacingM * 2
            radius: Theme.cornerRadius
            color: root.isOn ? Theme.primary : Theme.surfaceContainerHigh

            StyledText {
                anchors.centerIn: parent
                text: "T"
                font.pixelSize: Theme.fontSizeMedium
                color: root.isOn ? Theme.onPrimary : Theme.surfaceText
            }
        }
    }

    pillClickAction: () => {
        if (root.screenState === "absent")
            return;
        toggleProc.running = true;
    }

    popoutWidth: 380
    popoutHeight: 340

    popoutContent: Component {
        PopoutComponent {
            headerText: "Tablet screen"
            detailsText: root.screenState === "absent" ? "no tablet attached (or the daemon is not running)" : ("virtual output is " + (root.isOn ? "on" : "off"))

            Column {
                width: parent.width
                spacing: Theme.spacingM

                Row {
                    spacing: Theme.spacingM

                    StyledRect {
                        width: 110
                        height: 34
                        radius: Theme.cornerRadius
                        color: root.isOn ? Theme.primary : Theme.surfaceContainerHigh

                        StyledText {
                            anchors.centerIn: parent
                            text: root.isOn ? "Turn off" : "Turn on"
                            color: root.isOn ? Theme.onPrimary : Theme.surfaceText
                            font.pixelSize: Theme.fontSizeSmall
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: toggleProc.running = true
                        }
                    }

                    StyledRect {
                        width: 110
                        height: 34
                        radius: Theme.cornerRadius
                        color: Theme.surfaceContainerHigh

                        StyledText {
                            anchors.centerIn: parent
                            text: "Settings…"
                            color: Theme.surfaceText
                            font.pixelSize: Theme.fontSizeSmall
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.openSettings()
                        }
                    }
                }

                StyledText {
                    text: "Resolution (restarts the stream)"
                    color: Theme.surfaceVariantText
                    font.pixelSize: Theme.fontSizeSmall
                }

                Repeater {
                    model: root.presets

                    delegate: StyledRect {
                        required property var modelData
                        width: parent.width
                        height: 32
                        radius: Theme.cornerRadius
                        color: Theme.surfaceContainerHigh

                        StyledText {
                            anchors.left: parent.left
                            anchors.leftMargin: Theme.spacingM
                            anchors.verticalCenter: parent.verticalCenter
                            text: parent.modelData.label
                            color: Theme.surfaceText
                            font.pixelSize: Theme.fontSizeSmall
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.applyPreset(parent.modelData)
                        }
                    }
                }
            }
        }
    }
}
