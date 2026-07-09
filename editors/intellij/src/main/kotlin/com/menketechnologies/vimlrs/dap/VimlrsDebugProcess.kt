package com.menketechnologies.vimlrs.dap

import com.google.gson.JsonArray
import com.google.gson.JsonObject
import com.intellij.execution.process.ProcessHandler
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.diagnostic.Logger
import com.intellij.xdebugger.XDebugProcess
import com.intellij.xdebugger.XDebugSession
import com.intellij.xdebugger.breakpoints.XBreakpointHandler
import com.intellij.xdebugger.evaluation.XDebuggerEditorsProvider
import java.io.InputStream
import java.io.OutputStream

/**
 * [XDebugProcess] speaking DAP over the `viml --dap` server's stdio.
 *
 * Unlike a TCP adapter, vimlrs multiplexes the debuggee's own `:echo` /
 * `:echomsg` output as DAP `output` events on the same stdout that carries
 * the protocol frames — so the Debug Console is driven entirely by those
 * events ([handleEvent] → `processHandler.notifyTextAvailable`), NOT by
 * attaching to the process stdout (which is the protocol stream the DAP
 * client reads). [dapInput] / [dapOutput] are the launched process's
 * stdout / stdin.
 */
class VimlrsDebugProcess(
    session: XDebugSession,
    private val processHandler: ProcessHandler,
    private val dapInput: InputStream,
    private val dapOutput: OutputStream,
    private val programPath: String,
    private val programArgs: List<String>,
    private val workingDirectory: String?,
) : XDebugProcess(session) {

    @Volatile var client: VimlrsDapClient? = null
        private set

    private val executionStack = VimlrsExecutionStack()
    private val editorsProvider = VimlrsDebuggerEditorsProvider()
    private val breakpointHandlers = arrayOf<XBreakpointHandler<*>>(VimlrsBreakpointHandler(this))

    override fun getEditorsProvider(): XDebuggerEditorsProvider = editorsProvider
    override fun getBreakpointHandlers(): Array<XBreakpointHandler<*>> = breakpointHandlers
    override fun doGetProcessHandler(): ProcessHandler = processHandler

    override fun createConsole(): com.intellij.execution.ui.ExecutionConsole {
        val console = com.intellij.execution.filters.TextConsoleBuilderFactory
            .getInstance()
            .createBuilder(session.project)
            .console as com.intellij.execution.ui.ConsoleView
        console.attachToProcess(processHandler)
        return console
    }

    override fun sessionInitialized() {
        super.sessionInitialized()
        ApplicationManager.getApplication().invokeLater {
            if (!processHandler.isStartNotified) {
                processHandler.startNotify()
            }
        }

        val c = VimlrsDapClient(
            output = dapOutput,
            input = dapInput,
            onEvent = { ev, body -> handleEvent(ev, body) },
            onLog = { /* trace if needed */ },
        )
        client = c

        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                c.request(
                    "initialize",
                    JsonObject().apply {
                        addProperty("clientID", "intellij-vimlrs")
                        addProperty("clientName", "IntelliJ vimlrs")
                        addProperty("adapterID", "vimlrs")
                        addProperty("locale", "en-US")
                        addProperty("linesStartAt1", true)
                        addProperty("columnsStartAt1", true)
                        addProperty("pathFormat", "path")
                        addProperty("supportsVariableType", true)
                        addProperty("supportsRunInTerminalRequest", false)
                        addProperty("supportsProgressReporting", false)
                    },
                )
                sendAllBreakpoints()
                c.request("configurationDone")
                val launchArgs = JsonObject().apply {
                    addProperty("program", programPath)
                    addProperty("stopOnEntry", false)
                    val args = JsonArray()
                    programArgs.forEach { args.add(it) }
                    add("args", args)
                    workingDirectory?.let { addProperty("cwd", it) }
                }
                c.request("launch", launchArgs)
            } catch (t: Throwable) {
                LOG.warn("DAP init sequence failed", t)
            }
        }
    }

    private fun sendAllBreakpoints() {
        val byFile = mutableMapOf<String, MutableList<Int>>()
        val mgr = com.intellij.xdebugger.XDebuggerManager.getInstance(session.project).breakpointManager
        for (bp in mgr.getBreakpoints(VimlrsBreakpointType::class.java)) {
            if (!bp.isEnabled) continue
            val path = bp.fileUrl.removePrefix("file://")
            byFile.getOrPut(path) { mutableListOf() }.add(bp.line + 1)
        }
        val c = client ?: return
        for ((path, lines) in byFile) {
            val args = JsonObject().apply {
                add("source", JsonObject().apply { addProperty("path", path) })
                val arr = JsonArray()
                for (l in lines) {
                    arr.add(JsonObject().apply { addProperty("line", l) })
                }
                add("breakpoints", arr)
            }
            c.requestAsync("setBreakpoints", args)
        }
    }

    private fun handleEvent(event: String, body: JsonObject) {
        when (event) {
            "stopped" -> onStopped(body)
            "terminated" -> session.stop()
            "exited" -> session.stop()
            "output" -> {
                val text = body.get("output")?.asString ?: return
                val category = body.get("category")?.asString ?: "stdout"
                val outputType = when (category) {
                    "stderr" -> com.intellij.execution.process.ProcessOutputTypes.STDERR
                    "console" -> com.intellij.execution.process.ProcessOutputTypes.SYSTEM
                    else -> com.intellij.execution.process.ProcessOutputTypes.STDOUT
                }
                processHandler.notifyTextAvailable(text, outputType)
            }
            else -> { /* informational */ }
        }
    }

    private fun onStopped(body: JsonObject) {
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val c = client ?: return@executeOnPooledThread

                val stArgs = JsonObject().apply {
                    addProperty("threadId", 1)
                    addProperty("startFrame", 0)
                    addProperty("levels", 100)
                }
                val stBody = c.request("stackTrace", stArgs) ?: return@executeOnPooledThread
                val rawFrames = stBody.getAsJsonArray("stackFrames") ?: return@executeOnPooledThread
                if (rawFrames.size() == 0) return@executeOnPooledThread

                val builtFrames = mutableListOf<VimlrsStackFrame>()
                for (rf in rawFrames) {
                    val fo = rf.asJsonObject
                    val frameId = fo.get("id")?.asInt ?: 0
                    val frameName = fo.get("name")?.asString ?: "<frame>"
                    val frameFile = fo.getAsJsonObject("source")?.get("path")?.asString ?: ""
                    val frameLine = fo.get("line")?.asInt ?: 0

                    val scopesArgs = JsonObject().apply { addProperty("frameId", frameId) }
                    val scopesBody = c.request("scopes", scopesArgs)
                    val scopes = scopesBody?.getAsJsonArray("scopes")

                    val children = mutableListOf<VimlrsValue>()
                    if (scopes != null) {
                        for (s in scopes) {
                            val so = s.asJsonObject
                            val varRef = so.get("variablesReference")?.asInt ?: continue
                            if (varRef == 0) continue
                            val varsArgs = JsonObject().apply { addProperty("variablesReference", varRef) }
                            val varsBody = c.request("variables", varsArgs) ?: continue
                            val vars = varsBody.getAsJsonArray("variables") ?: continue
                            for (v in vars) {
                                val vo = v.asJsonObject
                                children += VimlrsValue(
                                    name = vo.get("name")?.asString ?: "?",
                                    repr = vo.get("value")?.asString ?: "",
                                    kind = vo.get("type")?.asString ?: "scalar",
                                    varRef = vo.get("variablesReference")?.asInt ?: 0,
                                    client = c,
                                )
                            }
                        }
                    }
                    builtFrames += VimlrsStackFrame(
                        client = c,
                        frameId = frameId,
                        name = frameName,
                        file = frameFile,
                        line = frameLine,
                        children = children,
                    )
                }

                executionStack.setFrames(builtFrames)
                val ctx = VimlrsSuspendContext(executionStack)
                ApplicationManager.getApplication().invokeLater {
                    session.positionReached(ctx)
                }
            } catch (t: Throwable) {
                LOG.warn("onStopped fetch failed", t)
            }
        }
    }

    override fun resume(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("continue", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startStepOver(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("next", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startStepInto(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("stepIn", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startStepOut(context: com.intellij.xdebugger.frame.XSuspendContext?) {
        client?.requestAsync("stepOut", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun startPausing() {
        client?.requestAsync("pause", JsonObject().apply { addProperty("threadId", 1) })
    }

    override fun stop() {
        client?.requestAsync("disconnect", JsonObject().apply { addProperty("terminateDebuggee", true) })
        client?.close()
        try { dapInput.close() } catch (_: Exception) {}
        if (!processHandler.isProcessTerminated) {
            try { processHandler.destroyProcess() } catch (_: Exception) {}
        }
    }

    override fun runToPosition(position: com.intellij.xdebugger.XSourcePosition, context: com.intellij.xdebugger.frame.XSuspendContext?) {
        val c = client ?: return
        val path = position.file.path
        val line = position.line + 1
        val args = JsonObject().apply {
            add("source", JsonObject().apply { addProperty("path", path) })
            val arr = JsonArray()
            arr.add(JsonObject().apply { addProperty("line", line) })
            add("breakpoints", arr)
        }
        c.requestAsync("setBreakpoints", args)
        c.requestAsync("continue", JsonObject().apply { addProperty("threadId", 1) })
    }

    companion object {
        private val LOG = Logger.getInstance(VimlrsDebugProcess::class.java)
    }
}
