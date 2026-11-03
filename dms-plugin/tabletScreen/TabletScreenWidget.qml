import QtQuick
import Quickshell
import Quickshell.Io
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

// The tablet screen's switch, its state and the presets that matter day to day.
//
// There is no window and no tray icon behind this: the daemon is a service and
// `uscreen display` / `uscreen config` is the whole interface. The pill shows
// whether the virtual output currently exists, which is exactly "is anything
// being rendered and encoded".
PluginComponent {
    id: root

    property var popoutService: null

    // "on" / "off" / "absent" (no daemon, or no tablet plugged in)
    property string screenState: "absent"
    property string mode: ""
    property string bitrate: ""

    readonly property var presets: [
        {
            "label": "2400×1500 @ 60",
            "spec": "2400x1500@60",
            "note": "default: keeps up, sharp"
        },
        {
            "label": "3000×1872 @ 50",
            "spec": "3000x1872@50",
            "note": "native, 50 Hz"
        },
        {
            "label": "3000×1872 @ 60",
            "spec": "3000x1872@60",
            "note": "native, encoder cannot keep up"
        }
    ]

    readonly property bool isOn: screenState === "on"

    Timer {
        interval: 3000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: {
            statusProc.running = true;
            configProc.running = true;
        }
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
        id: configProc
        command: ["sh", "-c", "uscreen config 2>/dev/null"]
        stdout: StdioCollector {
            onStreamFinished: {
                let width = "", height = "", fps = "", kbps = "";
                for (const line of this.text.split("\n")) {
                    const [key, value] = line.split("=").map(s => (s || "").trim());
                    if (key === "width")
                        width = value;
                    else if (key === "height")
                        height = value;
                    else if (key === "fps")
                        fps = value;
                    else if (key === "bitrate")
                        kbps = value;
                }
                root.mode = width ? (width + "×" + height + " @ " + fps) : "";
                root.bitrate = kbps;
            }
        }
    }

    Process {
        id: toggleProc
        command: ["uscreen", "display", "toggle"]
        onExited: statusProc.running = true
    }

    Process {
        id: actionProc
        onExited: {
            statusProc.running = true;
            configProc.running = true;
        }
    }

    function run(command) {
        actionProc.command = ["sh", "-c", command];
        actionProc.running = true;
    }

    horizontalBarPill: Component {
        // BasePill sizes itself from the content's implicit size, so the root
        // has to be an Item/Row that reports one. A Rectangle with an explicit
        // `width` reports implicitWidth 0, which is why the old pill collapsed
        // to padding and the text spilled out of it.
        Item {
            implicitWidth: row.implicitWidth
            implicitHeight: row.implicitHeight

            Row {
                id: row
                anchors.centerIn: parent
                spacing: Theme.spacingXS

                DankIcon {
                    name: "tablet_mac"
                    size: root.iconSize
                    color: root.isOn ? Theme.primary : Theme.widgetIconColor
                    anchors.verticalCenter: parent.verticalCenter
                }

                StyledText {
                    text: root.screenState === "absent" ? "—" : (root.isOn ? "on" : "off")
                    font.pixelSize: Theme.barTextSize(root.barThickness, root.barConfig?.fontScale, root.barConfig?.maximizeWidgetText)
                    color: root.isOn ? Theme.primary : Theme.widgetTextColor
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
        }
    }

    verticalBarPill: Component {
        Item {
            implicitWidth: column.implicitWidth
            implicitHeight: column.implicitHeight

            Column {
                id: column
                anchors.centerIn: parent
                spacing: 1

                DankIcon {
                    name: "tablet_mac"
                    size: root.iconSize
                    color: root.isOn ? Theme.primary : Theme.widgetIconColor
                    anchors.horizontalCenter: parent.horizontalCenter
                }

                StyledText {
                    text: root.screenState === "absent" ? "—" : (root.isOn ? "on" : "off")
                    font.pixelSize: Theme.barTextSize(root.barThickness, root.barConfig?.fontScale, root.barConfig?.maximizeWidgetText)
                    color: root.isOn ? Theme.primary : Theme.widgetTextColor
                    anchors.horizontalCenter: parent.horizontalCenter
                }
            }
        }
    }

    // No pillClickAction on purpose: BasePill then opens the popout on click,
    // which is where "what does this do" is answered (state, on/off, presets).
    // With an action set, the popout was unreachable and the pill looked like
    // it did nothing.

    popoutWidth: 400
    popoutHeight: 330

    popoutContent: Component {
        PopoutComponent {
            headerText: "Tablet screen"
            detailsText: root.screenState === "absent" ? "no tablet attached (or the daemon is not running)" : (root.mode + (root.bitrate ? " · " + (root.bitrate / 1000) + " Mbps" : ""))

            Column {
                width: parent.width
                spacing: Theme.spacingM

                Row {
                    spacing: Theme.spacingM

                    StyledRect {
                        id: toggleButton
                        width: 120
                        height: 34
                        radius: Theme.cornerRadius
                        color: root.screenState === "absent" ? Theme.surfaceContainerHigh : Theme.primary

                        StyledText {
                            anchors.centerIn: parent
                            text: root.screenState === "absent" ? "No tablet" : (root.isOn ? "Turn off" : "Turn on")
                            color: root.screenState === "absent" ? Theme.surfaceVariantText : Theme.onPrimary
                            font.pixelSize: Theme.fontSizeSmall
                        }

                        MouseArea {
                            anchors.fill: parent
                            enabled: root.screenState !== "absent"
                            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                            onClicked: toggleProc.running = true
                        }
                    }

                    StyledText {
                        anchors.verticalCenter: parent.verticalCenter
                        text: root.isOn ? "streaming" : "not streaming"
                        color: root.isOn ? Theme.primary : Theme.surfaceVariantText
                        font.pixelSize: Theme.fontSizeSmall
                    }
                }

                StyledText {
                    text: "Mode (applied live)"
                    color: Theme.surfaceVariantText
                    font.pixelSize: Theme.fontSizeSmall
                }

                Repeater {
                    model: root.presets

                    delegate: StyledRect {
                        id: presetRect
                        required property var modelData
                        width: parent.width
                        height: 40
                        radius: Theme.cornerRadius
                        color: Theme.surfaceContainerHigh

                        Column {
                            anchors.left: parent.left
                            anchors.leftMargin: Theme.spacingM
                            anchors.verticalCenter: parent.verticalCenter
                            spacing: 2

                            StyledText {
                                text: parent.parent.modelData.label
                                color: Theme.surfaceText
                                font.pixelSize: Theme.fontSizeSmall
                            }

                            StyledText {
                                text: parent.parent.modelData.note
                                color: Theme.surfaceVariantText
                                font.pixelSize: Theme.fontSizeSmall - 1
                            }
                        }

                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.run("uscreen config mode " + presetRect.modelData.spec)
                        }
                    }
                }
            }
        }
    }
}
