import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

ShellRoot {
    id: root

    property var items: []
    property var viewStack: []
    property var composeItem: null
    property int currentIndex: 0
    property string query: ""
    property string status: "Loading providers..."
    property bool closeAfterAction: false
    property var filteredItems: filterItems(items, query)

    function score(queryText, candidate) {
        const queryLower = queryText.toLowerCase()
        const candidateLower = candidate.toLowerCase()
        if (queryLower.length === 0)
            return 0
        if (queryLower === candidateLower)
            return 10000 - candidate.length
        if (candidateLower.startsWith(queryLower))
            return 7500 - candidate.length + queryLower.length * 20

        let scoreValue = 0
        let queryIndex = 0
        let lastMatch = -2
        for (let index = 0; index < candidateLower.length && queryIndex < queryLower.length; index++) {
            if (candidateLower[index] !== queryLower[queryIndex])
                continue
            scoreValue += 100
            if (index === 0)
                scoreValue += 60
            if (index === lastMatch + 1)
                scoreValue += 50
            if (index > 0 && " _-/.".includes(candidateLower[index - 1]))
                scoreValue += 40
            lastMatch = index
            queryIndex++
        }
        return queryIndex === queryLower.length ? scoreValue + queryLower.length * 40 - candidate.length * 2 : -1
    }

    function filterItems(source, queryText) {
        const ranked = []
        for (let index = 0; index < source.length; index++) {
            const item = source[index]
            const titleScore = score(queryText, item.title) + 600
            const subtitleScore = score(queryText, item.subtitle)
            const idScore = score(queryText, item.id) - 120
            const best = Math.max(titleScore, subtitleScore, idScore)
            if (best >= 0)
                ranked.push({ item: item, score: best, index: index })
        }
        ranked.sort((left, right) => right.score - left.score || left.index - right.index)
        return ranked.map(entry => entry.item)
    }

    function selectedItem() {
        return currentIndex >= 0 && currentIndex < filteredItems.length ? filteredItems[currentIndex] : null
    }

    function statusColor(statusText) {
        const normalized = statusText.toLowerCase()
        if (normalized.includes("failed") || normalized.includes("error"))
            return "#dc2626"
        if (normalized.includes("rejected"))
            return "#d97706"
        if (normalized.includes("completed"))
            return "#16a34a"
        return "#a3a3a3"
    }

    function receiveCatalog(text) {
        try {
            const payload = JSON.parse(text)
            items = payload.items
            currentIndex = 0
            status = payload.rejected.length > 0 ? `${payload.items.length} items, ${payload.rejected.length} rejected` : `${payload.items.length} items`
        } catch (error) {
            status = `Provider data error: ${error}`
            items = []
        }
    }

    function loadProvider(name, packageQuery) {
        query = ""
        composeItem = null
        const command = ["tuisual", "--json"]
        if (packageQuery && packageQuery.length > 0)
            command.push("--query", packageQuery)
        if (name)
            command.push(`--${name}`)
        status = "Loading..."
        catalogProcess.exec(command)
    }

    function appendFlags(action, subItem) {
        const suffix = subItem.flags.length > 0 ? ` ${subItem.flags.join(" ")}` : ""
        const exitAfter = subItem.exit_after === undefined || subItem.exit_after === null
            ? action.type === "shell_command_exit"
            : subItem.exit_after
        if (subItem.input) {
            return {
                type: "shell_command_with_flag",
                value: {
                    command: action.type === "shell_command_with_flag" ? action.value.command + suffix : action.value + suffix,
                    flag_prefix: subItem.input.flag_prefix,
                    prompt: subItem.input.prompt,
                    exit_after: exitAfter
                }
            }
        }
        if (action.type === "shell_command_with_flag") {
            return {
                type: "shell_command_with_flag",
                value: {
                    command: action.value.command + suffix,
                    flag_prefix: action.value.flag_prefix,
                    prompt: action.value.prompt,
                    exit_after: exitAfter
                }
            }
        }
        return { type: exitAfter ? "shell_command_exit" : "shell_command", value: action.value + suffix }
    }

    function openSubItems(parent) {
        const childItems = parent.sub_items.map(subItem => ({
            provider: parent.provider,
            id: `${parent.id}::${subItem.id}`,
            title: subItem.title,
            subtitle: subItem.subtitle.length > 0 ? subItem.subtitle : parent.subtitle,
            info: { summary: `Sub-item of '${parent.title}'.`, fields: parent.info.fields },
            action: appendFlags(parent.action, subItem),
            require_sub_item: subItem.require_sub_item,
            sub_items: subItem.sub_items
        }))
        viewStack = viewStack.concat([{ items: items, query: query }])
        items = childItems
        query = ""
        currentIndex = 0
        status = `Options for ${parent.title}`
    }

    function launch(item) {
        if (!item)
            return
        if (item.require_sub_item && item.sub_items.length > 0) {
            openSubItems(item)
            return
        }
        if (item.action.type === "provider_hint") {
            viewStack = []
            loadProvider(item.action.value, "")
            return
        }
        if (item.action.type === "shell_command_with_flag") {
            composeItem = item
            query = ""
            status = item.action.value.prompt
            input.forceActiveFocus()
            return
        }
        if (item.sub_items.length > 0) {
            openSubItems(item)
            return
        }
        runCommand(item.action.value, item.action.type === "shell_command_exit")
    }

    function confirmInput() {
        if (!composeItem)
            return
        const action = composeItem.action.value
        const value = query.trim()
        const command = value.length > 0 ? `${action.command} ${action.flag_prefix}${value}` : action.command
        composeItem = null
        query = ""
        runCommand(command, action.exit_after)
    }

    function runCommand(command, exitAfter) {
        closeAfterAction = exitAfter
        status = `Running: ${command}`
        actionProcess.exec(["sh", "-lc", command])
    }

    function goBack() {
        if (composeItem) {
            composeItem = null
            query = ""
            status = "Input cancelled"
            return
        }
        if (viewStack.length > 0) {
            const previous = viewStack[viewStack.length - 1]
            viewStack = viewStack.slice(0, -1)
            items = previous.items
            query = previous.query
            currentIndex = 0
            status = "Returned to results"
            return
        }
        Qt.quit()
    }

    Process {
        id: catalogProcess
        stdout: StdioCollector {
            onStreamFinished: root.receiveCatalog(text)
        }
        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0)
                root.status = `Provider command failed (${exitCode})`
        }
    }

    Process {
        id: actionProcess
        stdout: StdioCollector { }
        stderr: StdioCollector { id: actionStderr }
        onExited: (exitCode, exitStatus) => {
            root.status = exitCode === 0 ? "Action completed" : `Action failed (${exitCode}): ${actionStderr.text.trim()}`
            if (root.closeAfterAction && exitCode === 0)
                Qt.quit()
        }
    }

    PanelWindow {
        id: launcher
        anchors { top: true; bottom: true; left: true; right: true }
        exclusiveZone: 0
        color: "#00000000"
        focusable: true

        Rectangle {
            anchors.fill: parent
            color: "#000000b8"

            MouseArea {
                anchors.fill: parent
                onClicked: root.goBack()
            }

            Rectangle {
                id: surface
                width: Math.min(980, parent.width - 48)
                height: Math.min(610, parent.height - 48)
                anchors.centerIn: parent
                color: "#171717"
                border.color: "#a3a3a3"
                border.width: 2

                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Escape) {
                        root.goBack()
                        event.accepted = true
                    } else if (event.key === Qt.Key_Down) {
                        root.currentIndex = Math.min(root.currentIndex + 1, root.filteredItems.length - 1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Up) {
                        root.currentIndex = Math.max(root.currentIndex - 1, 0)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
                        if (root.composeItem)
                            root.confirmInput()
                        else
                            root.launch(root.selectedItem())
                        event.accepted = true
                    }
                }

                ColumnLayout {
                    anchors.fill: parent
                    anchors.margins: 24
                    spacing: 16

                    TextField {
                        id: input
                        Layout.fillWidth: true
                        Layout.preferredHeight: 52
                        focus: true
                        text: root.query
                        placeholderText: root.composeItem ? root.composeItem.action.value.prompt : "Search providers and actions"
                        font.family: "JetBrains Mono"
                        font.pixelSize: 18
                        color: "#f5f5f5"
                        selectByMouse: true
                        onTextEdited: {
                            root.query = text
                            root.currentIndex = 0
                        }
                        onAccepted: root.composeItem ? root.confirmInput() : root.launch(root.selectedItem())
                        background: Rectangle {
                            color: "#262626"
                            border.width: 2
                            border.color: input.activeFocus ? "#f5f5f5" : "#737373"
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: 20

                        ListView {
                            id: list
                            Layout.preferredWidth: surface.width * 0.54
                            Layout.fillHeight: true
                            clip: true
                            model: root.filteredItems
                            currentIndex: root.currentIndex
                            onCurrentIndexChanged: root.currentIndex = currentIndex
                            delegate: ItemDelegate {
                                required property int index
                                required property var modelData
                                width: list.width
                                height: 58
                                highlighted: index === root.currentIndex
                                text: modelData.title
                                font.family: "JetBrains Mono"
                                font.pixelSize: 16
                                onClicked: {
                                    root.currentIndex = index
                                    root.launch(modelData)
                                }
                                contentItem: Column {
                                    spacing: 3
                                    Text { text: modelData.title; font: parent.parent.font; color: "#f5f5f5"; elide: Text.ElideRight; width: parent.width }
                                    Text { text: modelData.subtitle; font.family: "JetBrains Mono"; font.pixelSize: 12; color: "#a3a3a3"; elide: Text.ElideRight; width: parent.width }
                                }
                                background: Rectangle {
                                    color: parent.highlighted ? "#404040" : "transparent"
                                }
                            }
                            ScrollBar.vertical: ScrollBar { }
                        }

                        Rectangle {
                            Layout.preferredWidth: surface.width * 0.38
                            Layout.fillHeight: true
                            color: "#262626"
                            border.color: "#737373"

                            Flickable {
                                anchors.fill: parent
                                anchors.margins: 18
                                contentWidth: width
                                contentHeight: details.implicitHeight
                                clip: true

                                Column {
                                    id: details
                                    width: parent.width
                                    spacing: 12
                                    property var selected: root.selectedItem()
                                    Text { text: details.selected ? details.selected.title : "No selection"; width: parent.width; wrapMode: Text.Wrap; font.family: "JetBrains Mono"; font.pixelSize: 19; font.bold: true; color: "#f5f5f5" }
                                    Text { text: details.selected ? details.selected.provider : ""; width: parent.width; wrapMode: Text.Wrap; font.family: "JetBrains Mono"; font.pixelSize: 12; color: "#d4d4d4" }
                                    Rectangle { width: parent.width; height: 1; color: "#737373" }
                                    Text { text: details.selected ? details.selected.info.summary : ""; width: parent.width; wrapMode: Text.Wrap; font.family: "JetBrains Mono"; font.pixelSize: 14; color: "#d4d4d4" }
                                    Repeater {
                                        model: details.selected ? details.selected.info.fields : []
                                        delegate: Text { required property var modelData; text: `${modelData.label}: ${modelData.value}`; width: parent.width; wrapMode: Text.Wrap; font.family: "JetBrains Mono"; font.pixelSize: 12; color: "#a3a3a3" }
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.status
                        font.family: "JetBrains Mono"
                        font.pixelSize: 12
                        color: root.statusColor(root.status)
                        elide: Text.ElideRight
                    }
                }
            }
        }
    }

    Component.onCompleted: loadProvider("", "")
}