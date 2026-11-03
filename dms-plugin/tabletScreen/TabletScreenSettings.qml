import QtQuick
import Quickshell
import Quickshell.Io
import qs.Common
import qs.Widgets
import qs.Modules.Plugins

// Everything the daemon can change, in the one place that shows it.
//
// Each control writes through `uscreen config set`, which applies the change to
// the running pipeline over the daemon's control socket — no restart, no black
// frame, no config file to edit by hand. The values shown are read back from
// the daemon, so the page never disagrees with what is actually running.
PluginSettings {
    id: root
    pluginId: "tabletScreen"

    property var values: ({})
    property string lastError: ""

    readonly property var bitrates: [15000, 20000, 25000, 35000, 50000]
    readonly property var scales: ["1.0", "1.25", "1.5", "1.75", "2.0"]

    Timer {
        interval: 4000
        running: true
        repeat: true
        triggeredOnStart: true
        onTriggered: readProc.running = true
    }

    Process {
        id: readProc
        command: ["sh", "-c", "uscreen config 2>/dev/null"]
        stdout: StdioCollector {
            onStreamFinished: {
                const out = {};
                for (const line of this.text.split("\n")) {
                    const at = line.indexOf("=");
                    if (at < 0)
                        continue;
                    out[line.slice(0, at).trim()] = line.slice(at + 1).trim();
                }
                if (Object.keys(out).length > 0)
                    root.values = out;
            }
        }
    }

    Process {
        id: writeProc
        stderr: StdioCollector {
            onStreamFinished: root.lastError = this.text.trim()
        }
        onExited: readProc.running = true
    }

    function set(key, value) {
        writeProc.command = ["uscreen", "config", "set", key, String(value)];
        writeProc.running = true;
    }

    function run(args) {
        writeProc.command = ["uscreen", "config"].concat(args);
        writeProc.running = true;
    }

    function value(key, fallback) {
        return root.values[key] !== undefined ? root.values[key] : fallback;
    }

    // A row of choices; the active one is highlighted. `key` empty means the
    // choice is a whole mode (`uscreen config mode WxH@FPS`) rather than one
    // key.
    component Choices: Row {
        id: choices

        property string label: ""
        property string key: ""
        property var options: []
        property var labels: options

        spacing: Theme.spacingS

        StyledText {
            anchors.verticalCenter: parent.verticalCenter
            width: 110
            text: choices.label
            color: Theme.surfaceVariantText
            font.pixelSize: Theme.fontSizeSmall
        }

        Repeater {
            model: choices.options

            delegate: StyledRect {
                id: chipRect
                required property var modelData
                required property int index

                readonly property bool active: choices.key === ""
                    ? (root.value("width", "") + "x" + root.value("height", "") + "@" + root.value("fps", "")) === String(modelData)
                    : String(root.value(choices.key, "")) === String(modelData)

                width: Math.max(46, chip.implicitWidth + Theme.spacingM)
                height: 30
                radius: Theme.cornerRadius
                color: chipRect.active ? Theme.primary : Theme.surfaceContainerHigh

                StyledText {
                    id: chip
                    anchors.centerIn: parent
                    text: String(choices.labels[chipRect.index])
                    color: chipRect.active ? Theme.onPrimary : Theme.surfaceText
                    font.pixelSize: Theme.fontSizeSmall
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        if (choices.key === "")
                            root.run(["mode", String(chipRect.modelData)]);
                        else
                            root.set(choices.key, chipRect.modelData);
                    }
                }
            }
        }
    }

    // A number with − and +; step is per key.
    component Stepper: Row {
        id: stepper

        property string label: ""
        property string key: ""
        property real step: 1
        property real minimum: 0
        property real maximum: 100000

        spacing: Theme.spacingS

        StyledText {
            anchors.verticalCenter: parent.verticalCenter
            width: 110
            text: stepper.label
            color: Theme.surfaceVariantText
            font.pixelSize: Theme.fontSizeSmall
        }

        Repeater {
            model: ["−", "+"]

            delegate: StyledRect {
                id: stepRect
                required property string modelData

                width: 34
                height: 30
                radius: Theme.cornerRadius
                color: Theme.surfaceContainerHigh

                StyledText {
                    anchors.centerIn: parent
                    text: stepRect.modelData
                    color: Theme.surfaceText
                    font.pixelSize: Theme.fontSizeMedium
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        const current = Number(root.value(stepper.key, 0));
                        const delta = stepRect.modelData === "+" ? stepper.step : -stepper.step;
                        const next = Math.min(stepper.maximum, Math.max(stepper.minimum, current + delta));
                        root.set(stepper.key, next);
                    }
                }
            }
        }

        StyledText {
            anchors.verticalCenter: parent.verticalCenter
            width: 70
            text: String(root.value(stepper.key, ""))
            color: Theme.surfaceText
            font.pixelSize: Theme.fontSizeSmall
        }
    }

    Column {
        width: parent.width
        spacing: Theme.spacingM

        StyledText {
            width: parent.width
            text: "The tablet screen is a virtual output the daemon creates on demand. Turning it off removes the output, so nothing is rendered, captured or encoded while it is not in use."
            color: Theme.surfaceVariantText
            font.pixelSize: Theme.fontSizeSmall
            wrapMode: Text.WordWrap
        }

        Choices {
            label: "Capture"
            key: "capture"
            options: ["wayland", "evdi"]
            labels: ["DMA-BUF", "EVDI (old)"]
        }

        Choices {
            label: "Side"
            key: "position"
            options: ["right", "left", "above", "below"]
        }

        Choices {
            label: "Scale"
            key: "scale"
            options: root.scales
        }

        Choices {
            label: "Cursor"
            key: "cursor"
            options: ["true", "false"]
            labels: ["shown", "hidden"]
        }

        Choices {
            label: "Mode"
            options: ["2400x1500@60", "3000x1872@50", "3000x1872@60"]
            labels: ["2400×1500 @60", "3000×1872 @50", "3000×1872 @60"]
        }

        StyledText {
            text: "Bitrate and quality"
            color: Theme.surfaceText
            font.pixelSize: Theme.fontSizeMedium
            font.weight: Font.Medium
        }

        Stepper {
            label: "Bitrate (kbps)"
            key: "bitrate"
            step: 5000
            minimum: 5000
            maximum: 60000
        }

        Stepper {
            label: "Quality (qp)"
            key: "quality"
            step: 1
            minimum: 12
            maximum: 32
        }

        StyledText {
            width: parent.width
            visible: root.lastError !== ""
            text: root.lastError
            color: Theme.error
            font.pixelSize: Theme.fontSizeSmall
            wrapMode: Text.WordWrap
        }
    }
}
