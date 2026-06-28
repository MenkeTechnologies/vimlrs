package com.menketechnologies.vimlrs.run

import com.intellij.execution.configurations.LocatableRunConfigurationOptions

class VimlrsRunConfigurationOptions : LocatableRunConfigurationOptions() {
    var scriptPath: String? by string()
    var scriptArgs: String? by string()
    var interpreterArgs: String? by string()
    var workingDirectory: String? by string()
    var disasm: Boolean by property(false)         // --disasm (fusevm bytecode listing)
}
