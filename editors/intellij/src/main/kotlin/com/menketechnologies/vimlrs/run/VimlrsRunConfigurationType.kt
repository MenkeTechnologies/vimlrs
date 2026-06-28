package com.menketechnologies.vimlrs.run

import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.ConfigurationType
import com.intellij.execution.configurations.RunConfiguration
import com.intellij.openapi.project.Project
import com.menketechnologies.vimlrs.VimlrsIcons
import javax.swing.Icon

class VimlrsRunConfigurationType : ConfigurationType {
    override fun getDisplayName(): String = "vimlrs"
    override fun getConfigurationTypeDescription(): String = "Run a viml script with vimlrs"
    override fun getIcon(): Icon = VimlrsIcons.FILE
    override fun getId(): String = "VIMLRS_RUN_CONFIGURATION"
    override fun getConfigurationFactories(): Array<ConfigurationFactory> = arrayOf(factory)

    val factory = object : ConfigurationFactory(this) {
        override fun getId(): String = "vimlrs"
        override fun createTemplateConfiguration(project: Project): RunConfiguration =
            VimlrsRunConfiguration(project, this, "vimlrs")
        override fun getOptionsClass(): Class<VimlrsRunConfigurationOptions> =
            VimlrsRunConfigurationOptions::class.java
    }

    companion object {
        fun getInstance(): VimlrsRunConfigurationType =
            com.intellij.execution.configurations.ConfigurationTypeUtil
                .findConfigurationType(VimlrsRunConfigurationType::class.java)
    }
}
