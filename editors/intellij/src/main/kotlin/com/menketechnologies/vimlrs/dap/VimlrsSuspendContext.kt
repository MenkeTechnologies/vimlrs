package com.menketechnologies.vimlrs.dap

import com.intellij.xdebugger.frame.XExecutionStack
import com.intellij.xdebugger.frame.XStackFrame
import com.intellij.xdebugger.frame.XSuspendContext

class VimlrsSuspendContext(private val stack: VimlrsExecutionStack) : XSuspendContext() {
    override fun getActiveExecutionStack(): XExecutionStack = stack
}

class VimlrsExecutionStack : XExecutionStack("Main") {

    @Volatile private var frames: List<VimlrsStackFrame> = emptyList()

    fun setFrames(newFrames: List<VimlrsStackFrame>) {
        frames = newFrames
    }

    override fun getTopFrame(): XStackFrame? = frames.firstOrNull()

    override fun computeStackFrames(firstFrameIndex: Int, container: XStackFrameContainer) {
        val slice = if (firstFrameIndex <= 0) frames else frames.drop(firstFrameIndex)
        container.addStackFrames(slice, true)
    }
}
