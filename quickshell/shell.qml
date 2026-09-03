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
    property bool packageManagerCatalog: false
    property bool packageSearchPending: false
    property bool closeAfterAction: false
    property bool infoFocused: false
    property bool terminalLaunchPending: false
    property string terminalEmulator: "alacritty"
    property var terminalProviders: ["arch-updates", "installer"]
    property var filteredItems: filterItems(items, query)
    // Mirrors ui.rs's render_results compose-mode overlay so the list doesn't keep showing stale sub-items while typing a flag value.
    property var composeDisplayItems: {
        if (!composeItem)
            return []
        const stateLine = (composeItem.require_sub_item && composeItem.sub_items.length > 0)
            ? "Required chain: Enter or Space continues"
            : (composeItem.sub_items.length > 0
                ? "Optional chain: Enter launches, Space continues"
                : "Final step: Enter launches")
        return [
            { title: "Input Mode", subtitle: composeItem.title },
            { title: `Prompt: ${composeItem.action.value.prompt}`, subtitle: "" },
            { title: stateLine, subtitle: "" }
        ]
    }

    function score(queryLower, candidateLower, candidateLength) {
        if (queryLower.length === 0)
            return 0
        if (queryLower === candidateLower)
            return 10000 - candidateLength
        if (candidateLower.startsWith(queryLower))
            return 7500 - candidateLength + queryLower.length * 20

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
        return queryIndex === queryLower.length ? scoreValue + queryLower.length * 40 - candidateLength * 2 : -1
    }

    function filterItems(source, queryText) {
        const queryLower = queryText.toLowerCase()
        if (packageSearchPending)
            return []
        if (queryText.trim().length === 0
            && source.length > 0
            && source.every(item => item.provider === "installer"))
            return []

        const ranked = []
        for (let index = 0; index < source.length; index++) {
            const item = source[index]
            const titleScore = score(queryLower, item._titleLower || item.title.toLowerCase(), item.title.length) + 600
            const subtitleScore = score(queryLower, item._subtitleLower || item.subtitle.toLowerCase(), item.subtitle.length)
            const idScore = score(queryLower, item._idLower || item.id.toLowerCase(), item.id.length) - 120
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

    function toggleFocus() {
        infoFocused = !infoFocused
        status = infoFocused ? "Focus: Info" : "Focus: Results"
    }

    function scrollInfoBy(amount) {
        const maxY = Math.max(0, infoFlickable.contentHeight - infoFlickable.height)
        infoFlickable.contentY = Math.max(0, Math.min(maxY, infoFlickable.contentY + amount))
    }

    function statusColor(statusText) {
        const normalized = statusText.toLowerCase()
        if (normalized.includes("failed") || normalized.includes("error"))
            return Theme.colorError
        if (normalized.includes("rejected"))
            return Theme.colorWarning
        if (normalized.includes("completed"))
            return Theme.colorSuccess
        return Theme.colorTextSecondary
    }

    function controlsHint() {
        return composeItem
            ? "Type: input | Enter: confirm | Space: continue chain | Esc: cancel"
            : "Tab: focus pane | Up/Down: active pane | PgUp/PgDn: info | Enter: launch | Esc: quit"
    }

    function receiveCatalog(text) {
        try {
            const payload = JSON.parse(text)
            items = payload.items.map(item => {
                item._titleLower = item.title.toLowerCase()
                item._subtitleLower = item.subtitle.toLowerCase()
                item._idLower = item.id.toLowerCase()
                return item
            })
            packageManagerCatalog = payload.items.length > 0
                && payload.items.every(item => item.provider === "installer")
            packageSearchPending = false
            currentIndex = 0
            const unknownFlagWarning = payload.rejected.find(entry => entry.includes("no providers matched requested flags"))
            if (unknownFlagWarning) {
                status = `Warning: ${unknownFlagWarning}`
            } else {
                status = payload.rejected.length > 0 ? `${payload.items.length} items, ${payload.rejected.length} rejected` : `${payload.items.length} items`
            }
        } catch (error) {
            status = `Provider data error: ${error}`
            items = []
        }
    }

    function loadProvider(name, packageQuery) {
        query = ""
        composeItem = null
        packageManagerCatalog = name === "installer"
        packageSearchPending = false
        const command = ["tuisual", "--json"]
        if (packageQuery && packageQuery.length > 0)
            command.push("--query", packageQuery)
        if (name)
            command.push(`--${name}`)
        status = "Loading..."
        catalogProcess.exec(command)
    }

    function searchPackages(packageQuery) {
        const trimmed = packageQuery.trim()
        if (trimmed.length === 0)
            return
        packageSearchPending = true
        status = "Searching..."
        catalogProcess.exec(["tuisual", "--json", "--query", trimmed, "--installer"])
    }

    // Forwards any CLI flags passed to the `quisual` wrapper (e.g. -P) straight to `tuisual --json`.
    function loadInitial() {
        query = ""
        composeItem = null
        packageManagerCatalog = false
        packageSearchPending = false
        const raw = Quickshell.env("TUISUAL_QS_ARGS")
        const extraArgs = raw ? raw.split(/\s+/).filter(arg => arg.length > 0) : []
        status = "Loading..."
        catalogProcess.exec(["tuisual", "--json"].concat(extraArgs))
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

    function discoverPathFlags(item) {
        if (pathFlagsProcess.running) {
            status = "Still discovering flags, please wait..."
            return
        }
        status = "Discovering flags..."
        pathFlagsProcess.pendingItem = item
        pathFlagsProcess.exec(["tuisual", "--path-flags", item.title])
    }

    function receivePathFlags(item, text) {
        try {
            const subItems = JSON.parse(text)
            if (subItems.length === 0) {
                status = `No sub-items discovered for '${item.title}'`
                return
            }
            openSubItems({
                provider: item.provider,
                id: item.id,
                title: item.title,
                subtitle: item.subtitle,
                info: item.info,
                action: item.action,
                require_sub_item: item.require_sub_item,
                sub_items: subItems
            })
        } catch (error) {
            status = `Path flag discovery error: ${error}`
        }
    }

    function buildChildItem(parent, subItem) {
        return {
            provider: parent.provider,
            id: `${parent.id}::${subItem.id}`,
            title: subItem.title,
            subtitle: subItem.subtitle.length > 0 ? subItem.subtitle : parent.subtitle,
            info: { summary: `Sub-item of '${parent.title}'.`, fields: parent.info.fields },
            action: appendFlags(parent.action, subItem),
            require_sub_item: subItem.require_sub_item,
            sub_items: subItem.sub_items
        }
    }

    function openSubItems(parent) {
        const childItems = parent.sub_items.map(subItem => buildChildItem(parent, subItem))
        viewStack = viewStack.concat([{ items: items, query: query }])
        items = childItems
        query = ""
        currentIndex = 0
        status = `Options for ${parent.title}`
    }

    function startCompose(item) {
        composeItem = item
        query = ""
        status = item.action.value.prompt
        input.forceActiveFocus()
    }

    // Auto-skips the sub-items menu when a required parent has exactly one option, recursing into it.
    function handleRequiredSubItems(item) {
        if (!item.require_sub_item || item.sub_items.length === 0)
            return false
        if (item.sub_items.length === 1) {
            const child = buildChildItem(item, item.sub_items[0])
            if (child.action.type === "shell_command_with_flag") {
                startCompose(child)
            } else if (!handleRequiredSubItems(child)) {
                launch(child)
            }
            return true
        }
        openSubItems(item)
        return true
    }

    function launch(item) {
        if (!item)
            return
        if (handleRequiredSubItems(item))
            return
        if (item.action.type === "provider_hint") {
            viewStack = []
            loadProvider(item.action.value, "")
            return
        }
        if (item.action.type === "shell_command_with_flag") {
            startCompose(item)
            return
        }
        if (item.sub_items.length > 0) {
            openSubItems(item)
            return
        }
        runCommand(item.action.value, item.action.type === "shell_command_exit", item.provider)
    }

    function composeCommand(item, value) {
        const action = item.action.value
        const trimmed = value.trim()
        return trimmed.length > 0 ? `${action.command} ${action.flag_prefix}${trimmed}` : action.command
    }

    function confirmInput() {
        if (!composeItem)
            return
        if (composeItem.require_sub_item && composeItem.sub_items.length > 0) {
            advanceCompose()
            return
        }
        const action = composeItem.action.value
        const provider = composeItem.provider
        const command = composeCommand(composeItem, query)
        composeItem = null
        query = ""
        runCommand(command, action.exit_after, provider)
    }

    // Chains into the composed item's own next sub_items (e.g. "type name then Space for URL step").
    function advanceCompose() {
        const state = composeItem
        if (!state)
            return
        const command = composeCommand(state, query)
        composeItem = null
        query = ""
        if (state.sub_items.length === 0) {
            runCommand(command, state.action.value.exit_after, state.provider)
            return
        }
        const parent = {
            provider: state.provider,
            id: `${state.id}::next`,
            title: state.title,
            subtitle: "Composed step",
            info: state.info,
            action: { type: state.action.value.exit_after ? "shell_command_exit" : "shell_command", value: command },
            require_sub_item: true,
            sub_items: state.sub_items
        }
        if (!handleRequiredSubItems(parent))
            openSubItems(parent)
    }

    function runCommand(command, exitAfter, provider) {
        closeAfterAction = exitAfter
        status = `Running: ${command}`
        if (terminalProviders.includes(provider)) {
            if (terminalLaunchPending)
                return
            terminalLaunchPending = true
            terminalProcess.command = [terminalEmulator, "-e", "sh", "-lc", command]
            terminalProcess.startDetached()
            status = `Launched in terminal: ${command}`
            if (exitAfter)
                Qt.quit()
            return
        }
        if (exitAfter) {
            // Fully detach into a new session so the launched app survives after Qt.quit() below.
            actionProcess.exec(["setsid", "-f", "sh", "-c", `${command} </dev/null >/dev/null 2>&1`])
            return
        }
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

    Timer {
        id: packageSearchTimer
        interval: 60
        repeat: false
        onTriggered: root.searchPackages(root.query)
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

    Process {
        id: terminalProcess
    }

    Process {
        id: pathFlagsProcess
        property var pendingItem: null
        stdout: StdioCollector {
            onStreamFinished: root.receivePathFlags(pathFlagsProcess.pendingItem, text)
        }
        onExited: (exitCode, exitStatus) => {
            if (exitCode !== 0)
                root.status = `Path flag discovery failed (${exitCode})`
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
            color: Qt.rgba(Theme.colorSurface.r, Theme.colorSurface.g, Theme.colorSurface.b, Theme.overlayOpacity)

            MouseArea {
                anchors.fill: parent
                onClicked: root.goBack()
            }

            Rectangle {
                id: surface
                width: Math.min(980, parent.width - 48)
                height: Math.min(610, parent.height - 48)
                anchors.centerIn: parent
                radius: Theme.cornerRadius
                color: Theme.colorSurface
                border.color: Theme.colorBorder
                border.width: Theme.borderWidth

                Keys.priority: Keys.BeforeItem
                Keys.onPressed: event => {
                    if (event.key === Qt.Key_Escape) {
                        root.goBack()
                        event.accepted = true
                    } else if (event.key === Qt.Key_Down) {
                        if (root.infoFocused)
                            root.scrollInfoBy(20)
                        else
                            root.currentIndex = Math.min(root.currentIndex + 1, root.filteredItems.length - 1)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Up) {
                        if (root.infoFocused)
                            root.scrollInfoBy(-20)
                        else
                            root.currentIndex = Math.max(root.currentIndex - 1, 0)
                        event.accepted = true
                    } else if (event.key === Qt.Key_PageDown && root.infoFocused) {
                        root.scrollInfoBy(160)
                        event.accepted = true
                    } else if (event.key === Qt.Key_PageUp && root.infoFocused) {
                        root.scrollInfoBy(-160)
                        event.accepted = true
                    } else if (event.key === Qt.Key_Home && root.infoFocused) {
                        infoFlickable.contentY = 0
                        event.accepted = true
                    } else if (event.key === Qt.Key_End && root.infoFocused) {
                        infoFlickable.contentY = Math.max(0, infoFlickable.contentHeight - infoFlickable.height)
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
                    anchors.margins: Theme.outerMargin
                    spacing: Theme.panelSpacing

                    TextField {
                        id: input
                        Layout.fillWidth: true
                        implicitHeight: contentHeight + 2 * Theme.itemPadding
                        padding: Theme.itemPadding
                        focus: true
                        text: root.query
                        placeholderText: root.composeItem ? root.composeItem.action.value.prompt : "Search providers and actions"
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.fontSize
                        color: Theme.colorTextPrimary
                        selectByMouse: true
                        onTextEdited: {
                            root.query = text
                            root.currentIndex = 0
                            if (root.packageManagerCatalog && !root.composeItem) {
                                if (text.trim().length > 0) {
                                    root.packageSearchPending = true
                                    packageSearchTimer.restart()
                                } else {
                                    root.packageSearchPending = false
                                    packageSearchTimer.stop()
                                }
                            }
                        }
                        Keys.priority: Keys.BeforeItem
                        Keys.onPressed: event => {
                            if (event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab) {
                                root.toggleFocus()
                                event.accepted = true
                                return
                            }
                            if (event.key !== Qt.Key_Space)
                                return
                            if (root.composeItem) {
                                if (root.composeItem.sub_items.length > 0) {
                                    root.advanceCompose()
                                    event.accepted = true
                                }
                                return
                            }
                            const selected = root.selectedItem()
                            if (selected && selected.sub_items.length > 0) {
                                root.openSubItems(selected)
                                event.accepted = true
                            } else if (selected && selected.provider === "path-launcher") {
                                root.discoverPathFlags(selected)
                                event.accepted = true
                            }
                        }
                        background: Rectangle {
                            radius: Theme.cornerRadius
                            color: Theme.colorPanel
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: Theme.panelSpacing

                        ColumnLayout {
                            Layout.preferredWidth: surface.width * 0.54
                            Layout.fillHeight: true
                            spacing: 4

                            Text {
                                text: root.infoFocused ? "Results" : "Results [Focus]"
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.fontSize
                                font.bold: !root.infoFocused
                                color: root.infoFocused ? Theme.colorTextSecondary : Theme.colorTextPrimary
                            }

                            ListView {
                                id: list
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                clip: true
                                model: root.composeItem ? root.composeDisplayItems : root.filteredItems
                                currentIndex: root.composeItem ? -1 : root.currentIndex
                                onCurrentIndexChanged: {
                                    if (!root.composeItem)
                                        root.currentIndex = currentIndex
                                }
                                delegate: ItemDelegate {
                                    required property int index
                                    required property var modelData
                                    width: list.width
                                    padding: Theme.itemPadding
                                    highlighted: !root.composeItem && index === root.currentIndex
                                    text: modelData.title
                                    font.family: Theme.fontFamily
                                    font.pixelSize: Theme.fontSize
                                    enabled: !root.composeItem
                                    onClicked: {
                                        if (root.composeItem)
                                            return
                                        root.currentIndex = index
                                        root.launch(modelData)
                                    }
                                    contentItem: Column {
                                        spacing: 1
                                        Text { text: modelData.title; font: parent.parent.font; color: Theme.colorTextPrimary; elide: Text.ElideRight; width: parent.width }
                                        Text { text: modelData.subtitle; font.family: Theme.fontFamily; font.pixelSize: Theme.fontSize; color: Theme.colorTextSecondary; elide: Text.ElideRight; width: parent.width }
                                    }
                                    background: Rectangle {
                                        radius: Theme.cornerRadius
                                        color: parent.highlighted ? Theme.colorHighlight : "transparent"
                                    }
                                }
                                ScrollBar.vertical: ScrollBar { }
                            }
                        }

                        ColumnLayout {
                            Layout.preferredWidth: surface.width * 0.38
                            Layout.fillHeight: true
                            spacing: 4

                            Text {
                                text: root.infoFocused ? "Info [Focus]" : "Info"
                                font.family: Theme.fontFamily
                                font.pixelSize: Theme.fontSize
                                font.bold: root.infoFocused
                                color: root.infoFocused ? Theme.colorTextPrimary : Theme.colorTextSecondary
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.fillHeight: true
                                radius: Theme.cornerRadius
                                color: Theme.colorPanel

                                Flickable {
                                    id: infoFlickable
                                    anchors.fill: parent
                                    anchors.margins: Theme.panelPadding
                                    contentWidth: width
                                    contentHeight: details.implicitHeight
                                    clip: true

                                    Column {
                                        id: details
                                        width: parent.width
                                        spacing: 12
                                        property var selected: root.selectedItem()
                                        Text {
                                            visible: root.status.startsWith("Warning:")
                                            text: root.status
                                            width: parent.width
                                            wrapMode: Text.Wrap
                                            font.family: Theme.fontFamily
                                            font.pixelSize: Theme.fontSize
                                            font.bold: true
                                            color: Theme.colorWarning
                                        }
                                        Text { text: details.selected ? details.selected.title : "No selection"; width: parent.width; wrapMode: Text.Wrap; font.family: Theme.fontFamily; font.pixelSize: Theme.fontSize; font.bold: true; color: Theme.colorTextPrimary }
                                        Text { text: details.selected ? details.selected.provider : ""; width: parent.width; wrapMode: Text.Wrap; font.family: Theme.fontFamily; font.pixelSize: Theme.fontSize; color: Theme.colorTextTertiary }
                                        Rectangle { width: parent.width; height: 1; color: Theme.colorTextMuted }
                                        Text { text: details.selected ? details.selected.info.summary : ""; width: parent.width; wrapMode: Text.Wrap; font.family: Theme.fontFamily; font.pixelSize: Theme.fontSize; color: Theme.colorTextTertiary }
                                        Repeater {
                                            model: details.selected ? details.selected.info.fields : []
                                            delegate: Text { required property var modelData; text: `${modelData.label}: ${modelData.value}`; width: parent.width; wrapMode: Text.Wrap; font.family: Theme.fontFamily; font.pixelSize: Theme.fontSize; color: Theme.colorTextSecondary }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.status
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.fontSize
                        color: root.statusColor(root.status)
                        elide: Text.ElideRight
                    }

                    Text {
                        Layout.fillWidth: true
                        text: root.controlsHint()
                        font.family: Theme.fontFamily
                        font.pixelSize: Theme.fontSize
                        color: Theme.colorTextMuted
                        elide: Text.ElideRight
                    }
                }
            }
        }
    }

    Component.onCompleted: loadInitial()
}