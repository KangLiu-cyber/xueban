package com.xueban.app

import android.content.Context
import android.content.SharedPreferences
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import java.time.LocalDate
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit

/** 全局应用状态：登录态、学习空间、刷题、错题、组卷、模考。token 持久化到 SharedPreferences。 */
class AppState(context: Context) {
    private val prefs: SharedPreferences =
        context.getSharedPreferences("xueban", Context.MODE_PRIVATE)

    // ---- 会话 ----
    private val tokenState = mutableStateOf(prefs.getString("token", null))
    var token: String?
        get() = tokenState.value
        set(value) {
            tokenState.value = value
            Api.authToken = value
            if (value == null) prefs.edit().remove("token").apply()
            else prefs.edit().putString("token", value).apply()
        }
    var user by mutableStateOf<UserDto?>(null)
    var loggedIn by mutableStateOf(false)

    // ---- 学习空间 ----
    var workspace by mutableStateOf<Workspace?>(null)
    var tree by mutableStateOf<List<ItemNode>>(emptyList())
    var selectedEpId by mutableStateOf<Long?>(null)
    var currentItem by mutableStateOf<ItemBundle?>(null)
    var noteOpen by mutableStateOf(false)
    var annoMode by mutableStateOf(false)
    var annoQuote by mutableStateOf("")
    var annoText by mutableStateOf("")
    var annoDetail by mutableStateOf<Annotation?>(null)

    // ---- 刷题 ----
    var quizScope by mutableStateOf("全部范围")
    var quizScopeId by mutableStateOf<Long?>(null)
    var quizPool by mutableStateOf<List<QuestionBrief>>(emptyList())
    var quizIdx by mutableStateOf(0)
    var quizRight by mutableStateOf(0)
    var quizWrong by mutableStateOf(0)
    var quizAnswered by mutableStateOf(false)
    var quizOutcome by mutableStateOf<AnswerOutcome?>(null)
    var quizPicked by mutableStateOf(-1)

    // ---- 错题本 ----
    var wrongList by mutableStateOf<List<WrongListItem>>(emptyList())
    var redoIdx by mutableStateOf(-1)
    var redoAnswered by mutableStateOf(false)
    var redoPicked by mutableStateOf(-1)
    var redoOutcome by mutableStateOf<AnswerOutcome?>(null)

    // ---- 组卷 ----
    var asmSource by mutableStateOf("全部")
    var asmType by mutableStateOf("全部题型")
    var asmScope by mutableStateOf("全部集数")
    var asmScopeId by mutableStateOf<Long?>(null)
    var asmCount by mutableStateOf(75)
    var previewPaper by mutableStateOf<PaperBundle?>(null)

    // ---- 模考 ----
    var mockPaper by mutableStateOf<PaperBundle?>(null)
    var mockAnswers by mutableStateOf<Map<Int, JsonArray>>(emptyMap())
    var mockIdx by mutableStateOf(0)
    var mockSecs by mutableStateOf(150 * 60)
    var mockResult by mutableStateOf<PaperResult?>(null)
    var mockDots by mutableStateOf(emptyList<Boolean>())

    // ---- 我的 ----
    var credential by mutableStateOf<CredentialResponse?>(null)
    var examGoal by mutableStateOf("")
    var examDate by mutableStateOf("")

    // ---- Sheet / 弹层 ----
    var goalSheet by mutableStateOf(false)
    var epSheet by mutableStateOf(false)
    var annoSheet by mutableStateOf(false)
    var previewSheet by mutableStateOf(false)
    var resultSheet by mutableStateOf(false)
    var confirmLogout by mutableStateOf(false)
    var freshEnter by mutableStateOf(false)

    // ---- Agent 接入 ----
    var agentSheetOpen by mutableStateOf(false)
    var agentConfigText by mutableStateOf("")

    // ---- 导航 ----
    var tab by mutableStateOf(0)
    var toastMsg by mutableStateOf<String?>(null)
    var busy by mutableStateOf(false)

    // ---- 目标 / 日期 ----
    var goalInput by mutableStateOf("")
    var dateInput by mutableStateOf("2026-11-07")

    init {
        Api.authToken = token
    }

    // ==================== 通用 ====================

    fun toast(msg: String) {
        toastMsg = msg
    }

    fun <T> guard(label: String, block: () -> T): T? = try {
        busy = true
        block().also { busy = false }
    } catch (e: Api.ApiException) {
        busy = false
        toast(e.message ?: label)
        if (e.code == 401) resetSession()
        null
    } catch (e: Exception) {
        busy = false
        toast("网络错误：${e.message ?: "无法连接服务器"}")
        null
    }

    fun resetSession() {
        token = null
        user = null
        loggedIn = false
        workspace = null
        tree = emptyList()
        currentItem = null
        noteOpen = false
        wrongList = emptyList()
        quizPool = emptyList()
        mockPaper = null
        tab = 0
    }

    fun logout() {
        runCatching { Api.logout() }
        resetSession()
    }

    // ==================== 登录 / 注册 ====================

    fun register(account: String, password: String): Boolean {
        val resp = guard("注册失败") { Api.register(account, password) } ?: return false
        token = resp.token
        user = resp.user
        return true
    }

    fun login(account: String, password: String): Boolean {
        val resp = guard("登录失败") { Api.login(account, password) } ?: return false
        token = resp.token
        user = resp.user
        return true
    }

    // ==================== 学习空间 ====================

    fun ensureWorkspace(name: String, goal: String, date: String?) {
        val ws = guard("创建空间失败") {
            val list = Api.listWorkspaces()
            if (list.isEmpty()) {
                Api.createWorkspace(
                    WorkspaceInput(name = name, examGoal = goal, examDate = date)
                )
            } else {
                Api.updateWorkspace(
                    list.first().id,
                    WorkspaceInput(name = list.first().name, examGoal = goal, examDate = date)
                )
            }
        } ?: return
        workspace = ws
        examGoal = ws.examGoal
        examDate = ws.examDate ?: ""
        loadTree()
        loggedIn = true
        freshEnter = true
    }

    fun loadTree() {
        val ws = workspace ?: return
        guard("加载目录失败") { Api.tree(ws.id) }?.let { tree = it }
    }

    /** 从目录树递归解析 note 的集数名（QuestionBrief 只带 source_item_id）。 */
    fun epNameOf(itemId: Long): String {
        fun walk(nodes: List<ItemNode>): String? {
            for (n in nodes) {
                if (n.item.id == itemId) return n.item.name
                walk(n.children)?.let { return it }
            }
            return null
        }
        return walk(tree) ?: "第${itemId}集"
    }

    /** 全部 note 叶子（树形拍平），供范围选择。 */
    fun allNotes(): List<ItemNode> {
        fun walk(nodes: List<ItemNode>, out: MutableList<ItemNode>) {
            for (n in nodes) {
                if (n.item.kind == ItemKind.Note) out.add(n)
                walk(n.children, out)
            }
        }
        val out = mutableListOf<ItemNode>()
        walk(tree, out)
        return out
    }

    fun openNote(itemId: Long) {
        val bundle = guard("加载笔记失败") { Api.itemBundle(itemId) } ?: return
        currentItem = bundle
        selectedEpId = itemId
        noteOpen = true
        annoMode = false
    }

    fun closeNote() {
        noteOpen = false
        annoMode = false
        selectedEpId = null
    }

    fun saveAnnotation(text: String) {
        val item = currentItem?.item ?: return
        val ok = guard("保存批注失败") {
            Api.addAnnotation(item.id, AnnotationInput(anchor = annoQuote, text = text))
        }
        if (ok != null) {
            openNote(item.id) // 重新拉取批注列表
            toast("批注已保存 · 点击批注可查看详情")
        }
    }

    fun deleteAnnotation(id: Long) {
        guard("删除失败") { Api.deleteAnnotation(id) }
        currentItem?.let { openNote(it.item.id) }
        annoDetail = null
        toast("批注已删除")
    }

    fun daysLeft(): Long {
        if (examDate.isBlank()) return 0
        return runCatching {
            ChronoUnit.DAYS.between(
                LocalDate.now(),
                LocalDate.parse(examDate, DateTimeFormatter.ISO_LOCAL_DATE)
            )
        }.getOrElse { 0 }.coerceAtLeast(0)
    }

    // ==================== 刷题 ====================

    fun loadQuiz(scopeId: Long? = null, scopeName: String = "全部范围") {
        val ws = workspace ?: return
        quizScope = scopeName
        quizScopeId = scopeId
        val pool = guard("加载题目失败") {
            Api.draw(ws.id, scope = scopeId, count = 10)
        } ?: return
        quizPool = pool
        quizIdx = 0
        quizRight = 0
        quizWrong = 0
        quizAnswered = false
        quizOutcome = null
        quizPicked = -1
    }

    fun currentQuestion(): QuestionBrief? = quizPool.getOrNull(quizIdx)

    /** 选项索引 → wire chosen：single→数字、multi→索引数组、judge→布尔。 */
    fun wireChosen(q: QuestionBrief, idx: Int): JsonElement = when (q.qtype) {
        QuestionType.Judge -> JsonPrimitive(q.options.getOrElse(idx) { "" } == "正确")
        else -> JsonPrimitive(idx)
    }

    fun submitAnswer(chosenIdx: Int) {
        val q = currentQuestion() ?: return
        submitAnswerWire(q, wireChosen(q, chosenIdx))
        quizPicked = chosenIdx
    }

    fun submitAnswerMulti(selected: List<Int>) {
        val q = currentQuestion() ?: return
        submitAnswerWire(q, JsonArray(selected.map { JsonPrimitive(it) }))
        quizPicked = selected.firstOrNull() ?: -1
    }

    private fun submitAnswerWire(q: QuestionBrief, chosen: JsonElement) {
        quizAnswered = true
        val outcome = guard("提交答案失败") {
            Api.answer(
                AnswerRequest(
                    questionId = q.id,
                    chosen = chosen,
                    scope = quizScope.takeUnless { it == "全部范围" },
                )
            )
        } ?: return
        quizOutcome = outcome
        if (outcome.isCorrect) quizRight++ else {
            quizWrong++
            toast("回答错误 · 已自动加入错题本")
            loadWrong()
        }
    }

    fun nextQuestion() {
        if (quizIdx < quizPool.size - 1) {
            quizIdx++
            quizAnswered = false
            quizOutcome = null
            quizPicked = -1
        }
    }

    // ==================== 错题本 ====================

    fun loadWrong() {
        guard("加载错题失败") { Api.wrongList() }?.let { wrongList = it }
    }

    fun redoWrong(index: Int) {
        redoIdx = index
        redoAnswered = false
        redoPicked = -1
        redoOutcome = null
    }

    fun submitRedo(chosenIdx: Int) {
        val item = wrongList.getOrNull(redoIdx) ?: return
        submitRedoWire(item, wireChosen(item.question, chosenIdx))
        redoPicked = chosenIdx
    }

    fun submitRedoMulti(selected: List<Int>) {
        val item = wrongList.getOrNull(redoIdx) ?: return
        submitRedoWire(item, JsonArray(selected.map { JsonPrimitive(it) }))
        redoPicked = selected.firstOrNull() ?: -1
    }

    private fun submitRedoWire(item: WrongListItem, chosen: JsonElement) {
        redoAnswered = true
        val outcome = guard("提交失败") {
            Api.answer(
                AnswerRequest(
                    questionId = item.question.id,
                    chosen = chosen,
                )
            )
        } ?: return
        redoOutcome = outcome
        if (outcome.isCorrect) toast("重做正确 ✓")
        else {
            toast("还是错了，再看一遍解析")
            val updated = wrongList.toMutableList()
            val w = updated[redoIdx]
            updated[redoIdx] = w.copy(wrong = w.wrong.copy(times = w.wrong.times + 1))
            wrongList = updated
        }
    }

    fun toggleMastered(index: Int) {
        val item = wrongList.getOrNull(index) ?: return
        val next = !item.wrong.mastered
        // P0-3：后端按 question_id 定位错题，传题目 id 而非 wrong.id
        // P1-9：取消掌握走 unmaster 端点，按钮双向可用
        guard("操作失败") {
            if (next) Api.markMastered(item.wrong.questionId)
            else Api.unmarkMastered(item.wrong.questionId)
        }
        val updated = wrongList.toMutableList()
        updated[index] = item.copy(wrong = item.wrong.copy(mastered = next))
        wrongList = updated
        toast(if (next) "已标记掌握" else "已取消掌握")
    }

    // ==================== 组卷 / 模考 ====================

    fun assemble(name: String = "架构师模拟卷 #1") {
        val ws = workspace ?: return
        val scope = when (asmScope) {
            "全部集数" -> null
            else -> asmScopeId?.toString()
        }
        val types = when (asmType) {
            "全部题型" -> null
            "单选题" -> listOf(QuestionType.Single)
            "多选题" -> listOf(QuestionType.Multi)
            else -> listOf(QuestionType.Judge)
        }
        val bundle = guard("组卷失败") {
            Api.assemblePaper(
                AssembleRequest(
                    workspaceId = ws.id,
                    name = name,
                    config = PaperConfig(
                        scope = scope,
                        questionTypes = types,
                        count = asmCount,
                    ),
                )
            )
        } ?: return
        previewPaper = bundle
    }

    fun startMock(bundle: PaperBundle) {
        mockPaper = bundle
        mockAnswers = emptyMap()
        mockIdx = 0
        mockSecs = 150 * 60
        mockResult = null
        mockDots = List(bundle.questions.size) { false }
    }

    fun mockPick(i: Int) {
        val q = mockPaper?.questions?.getOrNull(mockIdx) ?: return
        val cur = mockAnswers[mockIdx] ?: JsonArray(emptyList())
        mockAnswers = if (q.qtype == QuestionType.Multi) {
            val set = cur.toMutableList()
            val v = JsonPrimitive(i)
            if (set.contains(v)) set.remove(v) else set.add(v)
            mockAnswers + (mockIdx to JsonArray(set))
        } else {
            mockAnswers + (mockIdx to JsonArray(listOf(JsonPrimitive(i))))
        }
        mockDots = mockDots.toMutableList().also { it[mockIdx] = true }
    }

    fun mockPicked(i: Int): Boolean =
        (mockAnswers[mockIdx] ?: JsonArray(emptyList()))
            .contains(JsonPrimitive(i))

    fun mockAnswered(): Boolean = mockAnswers.containsKey(mockIdx)

    fun submitMock(onDone: () -> Unit) {
        val paper = mockPaper ?: return
        val answers = mockAnswers.entries.sortedBy { it.key }.map { (idx, chosen) ->
            val q = paper.questions.getOrNull(idx) ?: return@map null
            PaperAnswer(questionId = q.id, chosen = toWireChosen(q, chosen))
        }.filterNotNull()
        val result = guard("交卷失败") {
            Api.submitPaper(
                paper.paper.id,
                SubmitRequest(answers = answers, durationSecs = 150 * 60 - mockSecs),
            )
        } ?: return
        mockResult = result
        loadWrong()
        mockPaper = null
        onDone()
    }

    private fun toWireChosen(q: QuestionBrief, arr: JsonArray): JsonElement {
        val items = arr.map { it as JsonPrimitive }
        return when (q.qtype) {
            QuestionType.Multi -> JsonArray(items)
            QuestionType.Judge -> items.firstOrNull()?.let {
                JsonPrimitive(q.options.getOrElse(it.content.toInt()) { "" } == "正确")
            } ?: JsonPrimitive(false)
            else -> items.firstOrNull() ?: JsonPrimitive(0)
        }
    }

    // ==================== Agent 凭证 ====================

    fun loadCredential() {
        // P1-6：首次获取 404（尚无 agent token）时自动 rotate 签发
        val resp = guard("获取凭证失败") {
            try {
                Api.credential()
            } catch (e: Api.ApiException) {
                if (e.code == 404) Api.rotateCredential() else throw e
            }
        } ?: return
        credential = resp
        agentConfigText = buildAgentConfig(resp)
    }

    private fun buildAgentConfig(resp: CredentialResponse): String {
        val name = user?.nickname ?: user?.account ?: "同学"
        return "【超级学习助手 · Agent 接入凭证】\n" +
            "请代我接入以下 MCP 服务并完成装配：\n" +
            "  MCP 端点：${resp.endpoint}\n" +
            "  用户凭证：${resp.token}\n" +
            "  绑定用户：$name（考试目标：$examGoal）\n" +
            "接入后服务自动下发：Skill（笔记/习题/复盘）+ 备考提示词 + MCP 工具\n" +
            "装配完成后，请以我的名义与本系统交互。"
    }

    // ==================== 目标 ====================

    fun saveGoal(goal: String, date: String): Boolean {
        if (goal.isBlank()) {
            toast("请手写填写考试目标")
            return false
        }
        val ws = workspace ?: return false
        val updated = guard("保存失败") {
            Api.updateWorkspace(
                ws.id,
                WorkspaceInput(name = ws.name, examGoal = goal, examDate = date.ifBlank { null })
            )
        } ?: return false
        workspace = updated
        examGoal = updated.examGoal
        examDate = updated.examDate ?: ""
        toast("考试目标与日期已更新")
        return true
    }

    fun authSteps(): List<String> = listOf(
        "账号登录 ✓",
        "创建备考空间",
    )
}

/** 应用级单例状态（进程级复用，token 持久化）。 */
@Composable
fun rememberAppState(): AppState {
    val context = androidx.compose.ui.platform.LocalContext.current.applicationContext
    return androidx.compose.runtime.remember { AppState(context) }
}
