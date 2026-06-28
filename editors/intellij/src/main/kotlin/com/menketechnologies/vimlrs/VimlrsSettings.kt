package com.menketechnologies.vimlrs

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.util.xmlb.XmlSerializerUtil

@Service(Service.Level.APP)
@State(name = "VimlrsSettings", storages = [Storage("vimlrs.xml")])
class VimlrsSettings : PersistentStateComponent<VimlrsSettings.State> {
    data class State(
        var vimlrsExecutable: String? = null,
        var lspEnabled: Boolean = true,
        var extraLspArgs: String = "",
        var disableLexerHighlighting: Boolean = false,
        var fileExtensions: String = "vim",
        var autoRestartLsp: Boolean = true,
        var lspEnv: String = "",
        var logLspToFile: Boolean = false,
        var lspLogPath: String = "",
        var enableBuiltinHovers: Boolean = true,
    )

    private var stateData = State()

    override fun getState(): State = stateData
    override fun loadState(state: State) { XmlSerializerUtil.copyBean(state, stateData) }

    var vimlrsExecutable: String?
        get() = stateData.vimlrsExecutable
        set(value) { stateData.vimlrsExecutable = value }
    var lspEnabled: Boolean
        get() = stateData.lspEnabled
        set(value) { stateData.lspEnabled = value }
    var extraLspArgs: String
        get() = stateData.extraLspArgs
        set(value) { stateData.extraLspArgs = value }
    var disableLexerHighlighting: Boolean
        get() = stateData.disableLexerHighlighting
        set(value) { stateData.disableLexerHighlighting = value }
    var fileExtensions: String
        get() = stateData.fileExtensions
        set(value) { stateData.fileExtensions = value }
    var autoRestartLsp: Boolean
        get() = stateData.autoRestartLsp
        set(value) { stateData.autoRestartLsp = value }
    var lspEnv: String
        get() = stateData.lspEnv
        set(value) { stateData.lspEnv = value }
    var logLspToFile: Boolean
        get() = stateData.logLspToFile
        set(value) { stateData.logLspToFile = value }
    var lspLogPath: String
        get() = stateData.lspLogPath
        set(value) { stateData.lspLogPath = value }
    var enableBuiltinHovers: Boolean
        get() = stateData.enableBuiltinHovers
        set(value) { stateData.enableBuiltinHovers = value }

    fun supportedExtensions(): List<String> =
        fileExtensions.split(",", " ", ";")
            .map { it.trim().removePrefix(".") }
            .filter { it.isNotEmpty() }

    /** Match a virtual file's name/extension against the configured set. */
    fun isSupportedFile(filename: String, extension: String?): Boolean {
        if (extension != null && extension in supportedExtensions()) return true
        // Recognized Vim dotfile / rc bases regardless of extension.
        return filename in DOTFILES
    }

    companion object {
        // The vimrc / gvimrc / exrc / nvim family. `init.vim` also matches
        // by the `vim` extension, but is listed here so the bare-name path
        // (extension == null) still recognizes it.
        private val DOTFILES = setOf(
            "vimrc", ".vimrc", "_vimrc",
            "gvimrc", ".gvimrc", "_gvimrc",
            ".exrc", "_exrc",
            ".nvimrc", "init.vim",
        )

        fun getInstance(): VimlrsSettings =
            ApplicationManager.getApplication().getService(VimlrsSettings::class.java)
    }
}
