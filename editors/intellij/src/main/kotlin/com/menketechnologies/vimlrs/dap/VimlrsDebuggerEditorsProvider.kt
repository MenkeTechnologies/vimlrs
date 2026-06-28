package com.menketechnologies.vimlrs.dap

import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileTypes.FileType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiFileFactory
import com.intellij.xdebugger.XExpression
import com.intellij.xdebugger.XSourcePosition
import com.intellij.xdebugger.evaluation.EvaluationMode
import com.intellij.xdebugger.evaluation.XDebuggerEditorsProvider
import com.menketechnologies.vimlrs.VimlrsFileType

class VimlrsDebuggerEditorsProvider : XDebuggerEditorsProvider() {
    override fun getFileType(): FileType = VimlrsFileType

    override fun createDocument(
        project: Project,
        expression: XExpression,
        sourcePosition: XSourcePosition?,
        mode: EvaluationMode,
    ): Document {
        val psi = PsiFileFactory.getInstance(project).createFileFromText(
            "_vimlrs_expr.viml",
            VimlrsFileType,
            expression.expression,
        )
        return PsiDocumentManager.getInstance(project).getDocument(psi)
            ?: com.intellij.openapi.editor.EditorFactory.getInstance().createDocument(expression.expression)
    }
}
