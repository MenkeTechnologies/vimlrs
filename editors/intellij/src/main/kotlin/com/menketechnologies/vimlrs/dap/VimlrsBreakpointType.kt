package com.menketechnologies.vimlrs.dap

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.xdebugger.breakpoints.XLineBreakpointTypeBase
import com.menketechnologies.vimlrs.VimlrsSettings

/**
 * Line-breakpoint type for viml files. The runtime decides at execution time
 * whether a line is reachable; we accept any line of a supported file so the
 * gutter stays uniform.
 */
class VimlrsBreakpointType : XLineBreakpointTypeBase(
    "vimlrs-line",
    "vimlrs Line Breakpoint",
    VimlrsDebuggerEditorsProvider(),
) {
    override fun canPutAt(file: VirtualFile, line: Int, project: Project): Boolean =
        VimlrsSettings.getInstance().isSupportedFile(file.name, file.extension)

    override fun getPriority(): Int = 100
}
