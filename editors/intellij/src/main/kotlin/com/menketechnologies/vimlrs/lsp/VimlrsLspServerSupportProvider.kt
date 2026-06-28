package com.menketechnologies.vimlrs.lsp

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.LspServerSupportProvider.LspServerStarter
import com.menketechnologies.vimlrs.VimlrsSettings

class VimlrsLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(project: Project, file: VirtualFile, serverStarter: LspServerStarter) {
        val settings = VimlrsSettings.getInstance()
        if (!settings.lspEnabled) return
        if (!settings.isSupportedFile(file.name, file.extension)) return
        serverStarter.ensureServerStarted(VimlrsLspServerDescriptor(project))
    }
}
