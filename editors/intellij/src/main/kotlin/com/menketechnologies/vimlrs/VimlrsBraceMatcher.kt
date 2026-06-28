package com.menketechnologies.vimlrs

import com.intellij.lang.BracePair
import com.intellij.lang.PairedBraceMatcher
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IElementType

/**
 * Brace pairing for viml. Powers auto-insertion of `)`/`}`/`]` when
 * typing `(`/`{`/`[`, AND structural brace highlighting when the cursor
 * sits next to a paired delimiter.
 */
class VimlrsBraceMatcher : PairedBraceMatcher {
    private val pairs = arrayOf(
        BracePair(VimlrsTokenTypes.LPAREN, VimlrsTokenTypes.RPAREN, false),
        BracePair(VimlrsTokenTypes.LBRACE, VimlrsTokenTypes.RBRACE, true),
        BracePair(VimlrsTokenTypes.LBRACKET, VimlrsTokenTypes.RBRACKET, false),
    )

    override fun getPairs(): Array<BracePair> = pairs

    override fun isPairedBracesAllowedBeforeType(
        lbraceType: IElementType,
        contextType: IElementType?,
    ): Boolean = true

    override fun getCodeConstructStart(file: PsiFile?, openingBraceOffset: Int): Int =
        openingBraceOffset
}
