package com.menketechnologies.vimlrs

import org.junit.Assert.*
import org.junit.Test

/**
 * Pure-logic tests for [VimlrsSettings]. The `getInstance()` path touches
 * the IntelliJ ApplicationManager and must run under `gradle :test`
 * (BasePlatformTestCase); but the small parsers (`supportedExtensions`,
 * `isSupportedFile`) are pure on a freshly-constructed instance and can be
 * tested with plain JUnit.
 */
class VimlrsSettingsTest {

    private fun fresh(ext: String): VimlrsSettings {
        val s = VimlrsSettings()
        s.fileExtensions = ext
        return s
    }

    @Test fun `default extension set contains vim`() {
        val s = fresh("vim")
        assertTrue("vim" in s.supportedExtensions())
    }

    @Test fun `supportedExtensions parses comma list`() {
        val s = fresh("vim, vimrc,nvim; vimscript")
        val got = s.supportedExtensions().toSet()
        assertEquals(setOf("vim", "vimrc", "nvim", "vimscript"), got)
    }

    @Test fun `supportedExtensions strips leading dots`() {
        val s = fresh(".vim, .nvim")
        assertEquals(listOf("vim", "nvim"), s.supportedExtensions())
    }

    @Test fun `supportedExtensions ignores blanks`() {
        val s = fresh("  ,  vim ,,, ")
        assertEquals(listOf("vim"), s.supportedExtensions())
    }

    @Test fun `isSupportedFile matches by extension`() {
        val s = fresh("vim")
        assertTrue(s.isSupportedFile("plugin.vim", "vim"))
        assertTrue(s.isSupportedFile("init.vim", "vim"))
        assertFalse(s.isSupportedFile("readme.md", "md"))
    }

    @Test fun `isSupportedFile recognizes vimrc family without extension`() {
        val s = fresh("vim")
        for (name in listOf("vimrc", ".vimrc", "_vimrc", "gvimrc", ".gvimrc", "_gvimrc", ".exrc", "_exrc", ".nvimrc")) {
            assertTrue("$name should be supported", s.isSupportedFile(name, null))
        }
    }

    @Test fun `isSupportedFile rejects unrelated dotfiles`() {
        val s = fresh("vim")
        assertFalse(s.isSupportedFile(".bashrc", null))
        assertFalse(s.isSupportedFile(".zshrc", null))
        assertFalse(s.isSupportedFile(".profile", null))
    }
}
