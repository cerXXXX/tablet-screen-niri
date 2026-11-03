import QtQuick
import Quickshell
import Quickshell.Io
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

// Only the settings this widget is responsible for: which capture backend the
// daemon uses, where the tablet sits, and how much bandwidth the encoder may
// spend. Everything else (per-tablet profiles, pen mapping, the port numbers)
// stays in UScreen's own window, which this page links to rather than copies.
PluginSettings {
    id: root
    pluginId: "tabletScreen"

    readonly property string configTool: "$HOME/tablet-screen/tools/uscreen-config.py"

    property string currentValues: ""

    Timer {
        interval: 4000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: showProc.running = true
    }

    Process {
        id: showProc
        command: ["sh", "-c", root.configTool + " show 2>/dev/null"]
        stdout: StdioCollector {
            onStreamFinished: root.currentValues = this.text
        }
    }

    Process {
        id: setProc
        onExited: showProc.running = true
    }

    function setKey(key, value) {
        setProc.command = ["sh", "-c", root.configTool + " set " + key + " " + value + " && systemctl --user restart uscreen"];
        setProc.running = true;
    }

    function current(key) {
        const line = root.currentValues.split("\n").find(l => l.startsWith(key + " = "));
        return line ? line.slice(key.length + 3).trim() : "";
    }

    Column {
        width: parent.width
        spacing: Theme.spacingL

        StyledText {
            text: "Capture backend"
            color: Theme.surfaceText
            font.pixelSize: Theme.fontSizeMedium
            font.weight: Font.Medium
        }

        StyledText {
            width: parent.width
            text: "wayland is the zero-copy path: the compositor hands out its own GPU buffers and the iGPU encodes them (~8% of one core, 60 fps at 2400×1500). evdi is the old path through a kernel virtual display and a CPU copy."
            color: Theme.surfaceVariantText
            font.pixelSize: Theme.fontSizeSmall
            wrapMode: Text.WordWrap
        }

        Row {
            spacing: Theme.spacingM

            Repeater {
                model: ["wayland", "evdi"]

                delegate: StyledRect {
                    required property string modelData
                    width: 120
                    height: 34
                    radius: Theme.cornerRadius
                    color: root.current("capture").includes(modelData) ? Theme.primary : Theme.surfaceContainerHigh

                    StyledText {
                        anchors.centerIn: parent
                        text: parent.modelData
                        color: parent.color === Theme.primary ? Theme.onPrimary : Theme.surfaceText
                        font.pixelSize: Theme.fontSizeSmall
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.setKey("capture", parent.modelData)
                    }
                }
            }
        }

        StyledText {
            text: "Side of the desktop"
            color: Theme.surfaceText
            font.pixelSize: Theme.fontSizeMedium
            font.weight: Font.Medium
        }

        Row {
            spacing: Theme.spacingM

            Repeater {
                model: ["right", "left", "above", "below"]

                delegate: StyledRect {
                    required property string modelData
                    width: 80
                    height: 34
                    radius: Theme.cornerRadius
                    color: root.current("position").includes(modelData) ? Theme.primary : Theme.surfaceContainerHigh

                    StyledText {
                        anchors.centerIn: parent
                        text: parent.modelData
                        color: parent.color === Theme.primary ? Theme.onPrimary : Theme.surfaceText
                        font.pixelSize: Theme.fontSizeSmall
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.setKey("position", parent.modelData)
                    }
                }
            }
        }

        StyledText {
            text: "Bitrate"
            color: Theme.surfaceText
            font.pixelSize: Theme.fontSizeMedium
            font.weight: Font.Medium
        }

        StyledText {
            width: parent.width
            text: "Measured ceiling on this laptop is ~275 Mbps over USB, so bitrate is never the limit: 20 Mbps is enough for a sharp 2400×1500 desktop, 35 for native 3000×1872."
            color: Theme.surfaceVariantText
            font.pixelSize: Theme.fontSizeSmall
            wrapMode: Text.WordWrap
        }

        Row {
            spacing: Theme.spacingM

            Repeater {
                model: [20000, 35000, 50000]

                delegate: StyledRect {
                    required property int modelData
                    width: 100
                    height: 34
                    radius: Theme.cornerRadius
                    color: root.current("bitrate") === String(modelData) ? Theme.primary : Theme.surfaceContainerHigh

                    StyledText {
                        anchors.centerIn: parent
                        text: (parent.modelData / 1000) + " Mbps"
                        color: parent.color === Theme.primary ? Theme.onPrimary : Theme.surfaceText
                        font.pixelSize: Theme.fontSizeSmall
                    }

                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.setKey("bitrate", parent.modelData)
                    }
                }
            }
        }

        StyledRect {
            width: 200
            height: 34
            radius: Theme.cornerRadius
            color: Theme.surfaceContainerHigh

            StyledText {
                anchors.centerIn: parent
                text: "Open UScreen window…"
                color: Theme.surfaceText
                font.pixelSize: Theme.fontSizeSmall
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: Quickshell.execDetached(["uscreen-gui"])
            }
        }
    }
}
