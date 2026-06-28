package com.menketechnologies.vimlrs

import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

object VimlrsFileType : LanguageFileType(VimlrsLanguage) {
    override fun getName(): String = "VimL"
    override fun getDescription(): String = "VimL (Vimscript) script"
    override fun getDefaultExtension(): String = "vim"
    override fun getIcon(): Icon = VimlrsIcons.FILE
}
