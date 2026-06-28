package com.menketechnologies.vimlrs.actions

import com.intellij.ide.actions.CreateFileFromTemplateAction
import com.intellij.ide.actions.CreateFileFromTemplateDialog
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDirectory
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiFileFactory
import com.menketechnologies.vimlrs.VimlrsFileType
import com.menketechnologies.vimlrs.VimlrsIcons

/// File > New > VimL File. Hands the user a name dialog with a few canonical
/// starting templates (script, autoload, ftplugin, plain). All templates
/// resolve to `VimlrsFileType` so the new buffer immediately picks up syntax
/// highlighting, LSP, commenter, etc. without an IDE reload.
///
/// Implemented via the platform's `CreateFileFromTemplateAction` so we
/// inherit the standard New-File dialog (name field, template picker,
/// undoable PSI write). Templates are inline string literals here rather
/// than `fileTemplates/internal/*.vim` so the plugin stays single-jar with
/// no resource extraction at runtime.
class CreateVimlrsFileAction :
    CreateFileFromTemplateAction("VimL File", "Create new VimL script", VimlrsIcons.FILE) {

    override fun getActionName(directory: PsiDirectory?, newName: String, templateName: String?): String =
        "Create VimL File"

    override fun buildDialog(
        project: Project,
        directory: PsiDirectory,
        builder: CreateFileFromTemplateDialog.Builder,
    ) {
        builder
            .setTitle("New VimL File")
            .addKind("Script (#!/usr/bin/env vimlrs)", VimlrsIcons.FILE, TPL_SCRIPT)
            .addKind("Autoload",                        VimlrsIcons.FILE, TPL_AUTOLOAD)
            .addKind("Ftplugin",                        VimlrsIcons.FILE, TPL_FTPLUGIN)
            .addKind("Empty",                           VimlrsIcons.FILE, TPL_EMPTY)
    }

    override fun createFile(name: String, templateName: String, dir: PsiDirectory): PsiFile? {
        val fileName = if (name.contains('.')) name else "$name.vim"
        val body = when (templateName) {
            TPL_SCRIPT   -> SCRIPT_BODY
            TPL_AUTOLOAD -> AUTOLOAD_BODY
            TPL_FTPLUGIN -> FTPLUGIN_BODY
            else         -> ""
        }
        val file = PsiFileFactory.getInstance(dir.project)
            .createFileFromText(fileName, VimlrsFileType, body)
        return dir.add(file) as? PsiFile
    }

    companion object {
        private const val TPL_SCRIPT   = "Script"
        private const val TPL_AUTOLOAD = "Autoload"
        private const val TPL_FTPLUGIN = "Ftplugin"
        private const val TPL_EMPTY    = "Empty"

        private val SCRIPT_BODY = """
            |#!/usr/bin/env vimlrs
            |" vim:ft=vim
            |
            |function! s:main() abort
            |    echo "hello from vimlrs"
            |endfunction
            |
            |call s:main()
            |""".trimMargin()

        private val AUTOLOAD_BODY = """
            |" Autoload script — functions named after the file path are
            |" loaded on first use. Place under autoload/<name>.vim and call
            |" with <name>#Func().
            |
            |function! myplugin#greet(who) abort
            |    echomsg 'hello, ' .. a:who
            |endfunction
            |""".trimMargin()

        private val FTPLUGIN_BODY = """
            |" Filetype plugin — sourced when a buffer's filetype is set.
            |" Guard against double-sourcing per buffer.
            |if exists('b:did_ftplugin')
            |    finish
            |endif
            |let b:did_ftplugin = 1
            |
            |setlocal expandtab
            |setlocal shiftwidth=4
            |""".trimMargin()
    }
}
