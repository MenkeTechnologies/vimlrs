package com.menketechnologies.vimlrs

import com.intellij.lang.Language

object VimlrsLanguage : Language("vimlrs") {
    private fun readResolve(): Any = VimlrsLanguage
    override fun getDisplayName(): String = "vimlrs"
    override fun isCaseSensitive(): Boolean = true
}
