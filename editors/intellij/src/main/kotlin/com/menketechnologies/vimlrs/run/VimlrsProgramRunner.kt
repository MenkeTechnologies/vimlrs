package com.menketechnologies.vimlrs.run

import com.intellij.execution.configurations.RunProfile
import com.intellij.execution.executors.DefaultRunExecutor
import com.intellij.execution.runners.DefaultProgramRunner

class VimlrsProgramRunner : DefaultProgramRunner() {
    override fun getRunnerId(): String = "VimlrsProgramRunner"

    override fun canRun(executorId: String, profile: RunProfile): Boolean {
        if (profile !is VimlrsRunConfiguration) return false
        return executorId == DefaultRunExecutor.EXECUTOR_ID
    }
}
