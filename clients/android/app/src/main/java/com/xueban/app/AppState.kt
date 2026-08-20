package com.xueban.app

import android.app.Application
import android.content.Context
import android.content.SharedPreferences
import androidx.lifecycle.AndroidViewModel
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
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
    /** 启动时正在无感恢复登录（有本地 token，正在校验），用于避免登录页闪现。 */
    var restoring by mutableStateOf(false)

    // ---- 学习空间 ----
    var workspace by mutableStateOf<Workspace?>(null)
    /** 用户全部备考空间（切换空间入口用，登录/进入空间时刷新）。 */
    var workspaces by mutableStateOf<List<Workspace>>(emptyList())
    var tree by mutableStateOf<List<ItemNode>>(emptyList())
    var selectedEpId by mutableStateOf<Long?>(null)
    var currentItem by mutableStateOf<ItemBundle?>(null)
    var noteOpen by mutableStateOf(false)
    var annoMode by mutableStateOf(false)
    var annoQuote by mutableStateOf("")
    var annoText by mutableStateOf("")
    var annoDetail by mutableStateOf<Annotation?>(null)
    /** 已学记录：后端未下发 seen 字段，本地持久化已打开的笔记 id（原型「已学 / 未学」徽标）。 */
    var seenEpIds by mutableStateOf<Set<Long>>(emptySet())

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
    /** 已提交的视频作答题 id 集合（视频题不判分，提交即完成）。 */
    var videoSubmitted by mutableStateOf<Set<Long>>(emptySet())

    // ---- 错题本 ----
    var wrongList by mutableStateOf<List<WrongListItem>>(emptyList())
    var redoIdx by mutableStateOf(-1)
    var redoAnswered by mutableStateOf(false)
    var redoPicked by mutableStateOf(-1)
    var redoOutcome by mutableStateOf<AnswerOutcome?>(null)

    // ---- 组卷 ----
    var asmSource by mutableStateOf("全部")
    var asmType by mutableStateOf("单选题")
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
    /** 交卷错题明细（题干 / 用户所选 / 正确答案 / 解析），结果页展示「哪些题错了」。 */
    var mockWrongQuestions by mutableStateOf<List<WrongQuestionDetail>>(emptyList())
    var mockDots by mutableStateOf(emptyList<Boolean>())

    // ---- 我的 ----
    var credential by mutableStateOf<CredentialResponse?>(null)
    var examGoal by mutableStateOf("")
    var examDate by mutableStateOf("")
    var skills by mutableStateOf<List<SkillDto>>(emptyList())

    // ---- 自定义 Skill 表单 ----
    var skillName by mutableStateOf("")
    var skillDesc by mutableStateOf("")
    var skillScript by mutableStateOf("")

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
    var skillSheetOpen by mutableStateOf(false)

    // ---- 导航 ----
    var tab by mutableStateOf(0)
    var toastMsg by mutableStateOf<String?>(null)
    var busy by mutableStateOf(false)

    // ---- 目标 / 日期 ----
    var goalInput by mutableStateOf("")
    var dateInput by mutableStateOf("2026-11-07")

    /** §12.6：Room 缓存读写跑在 IO 协程，避免主线程访问数据库。 */
    private val cacheScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    init {
        Api.authToken = token
        CacheStore.init(context)
        // 有持久化 token 时启动即进入「无感恢复」态，避免登录页闪现。
        restoring = token != null
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
        workspaces = emptyList()
        tree = emptyList()
        currentItem = null
        noteOpen = false
        wrongList = emptyList()
        quizPool = emptyList()
        mockPaper = null
        seenEpIds = emptySet()
        tab = 0
    }

    /** §12.6：网络失败降级——异步读 Room 缓存，仅当当前列表为空时应用（避免旧缓存覆盖新数据）。 */
    private inline fun <reified T> cacheFallback(
        key: String,
        noinline empty: () -> Boolean,
        crossinline apply: (T) -> Unit,
        hint: String,
    ) {
        if (token == null) return
        cacheScope.launch {
            val cached = CacheStore.get<T>(key)
            withContext(Dispatchers.Main) {
                if (cached != null && empty()) {
                    apply(cached)
                    toast(hint)
                }
            }
        }
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
        recordLoginTime()
        return true
    }

    /**
     * 无感登录：启动时若有持久化 token，校验并直接进入已绑定空间，无需重新登录。
     * 凭证失效（401）时 `guard` 已调 resetSession 清 token，回到登录页；
     * 网络异常时保留 token 下次再试，本次回登录页。
     */
    fun restoreSession() {
        if (token == null) {
            restoring = false
            return
        }
        val resp = guard("恢复登录失败") { Api.me() }
        if (resp == null) {
            restoring = false
            return
        }
        user = resp.user
        recordLoginTime()
        enterExistingWorkspace()
        restoring = false
    }

    /** 记录最近一次登录/活跃时间（RFC3339 字符串，无感登录 & 记录登录时间用）。 */
    private fun recordLoginTime() {
        prefs.edit().putString("last_login_at", java.time.OffsetDateTime.now().toString()).apply()
    }

    // ==================== 学习空间 ====================

    /** 老用户登录：直接加载已绑定的备考空间并进入主界面（不重填目标、不弹 Agent 引导）。 */
    fun enterExistingWorkspace(): Boolean {
        val list = guard("加载空间失败") { Api.listWorkspaces() } ?: return false
        workspaces = list
        val ws = list.firstOrNull() ?: return false
        enterWorkspace(ws, fresh = false)
        return true
    }

    /** 首次登录：创建备考空间后进入主界面，并弹 Agent 接入引导。 */
    fun ensureWorkspace(name: String, goal: String, date: String?) {
        val ws = guard("创建空间失败") {
            val list = Api.listWorkspaces()
            if (list.isEmpty()) {
                Api.createWorkspace(
                    WorkspaceInput(name = name, examGoal = goal, examDate = date)
                )
            } else {
                // 已绑定空间的账号不应再进入本流程（防御：直接采用已有空间，不覆盖原目标）。
                list.first()
            }
        } ?: return
        enterWorkspace(ws, fresh = true)
        // 首次创建后只有一个空间，同步到切换列表
        workspaces = listOf(ws)
    }

    private fun enterWorkspace(ws: Workspace, fresh: Boolean) {
        workspace = ws
        examGoal = ws.examGoal
        examDate = ws.examDate ?: ""
        seenEpIds = prefs.getStringSet("seen_eps:${user?.id ?: ws.id}", emptySet())
            ?.mapNotNull { it.toLongOrNull() }?.toSet() ?: emptySet()
        loadTree()
        restoreMock()
        loggedIn = true
        freshEnter = fresh
    }

    /** 刷新用户空间列表（切换空间入口展示用）。 */
    fun loadWorkspaces() {
        val list = guard("加载空间失败") { Api.listWorkspaces() }
        if (list != null) workspaces = list
    }

    /** 切换备考空间：重置当前空间相关状态，加载新空间的内容树 / 题库 / 错题。 */
    fun switchWorkspace(ws: Workspace) {
        if (ws.id == workspace?.id) {
            goalSheet = false
            return
        }
        workspace = ws
        examGoal = ws.examGoal
        examDate = ws.examDate ?: ""
        // 重置与旧空间绑定的视图状态，避免串空间显示
        currentItem = null
        noteOpen = false
        annoMode = false
        annoDetail = null
        selectedEpId = null
        quizScope = "全部范围"
        quizScopeId = null
        quizPool = emptyList()
        quizIdx = 0
        quizRight = 0
        quizWrong = 0
        quizAnswered = false
        quizOutcome = null
        quizPicked = -1
        wrongList = emptyList()
        redoIdx = -1
        previewPaper = null
        mockPaper = null
        mockAnswers = emptyMap()
        mockResult = null
        mockWrongQuestions = emptyList()
        goalSheet = false
        loadTree()
        loadWrong()
        toast("已切换到「${ws.name}」")
    }

    /** 删除备考空间：删除后刷新列表；删的是当前空间则切到剩余第一个，无剩余则回登录。 */
    fun deleteWorkspace(ws: Workspace) {
        guard("删除失败") { Api.deleteWorkspace(ws.id) } ?: return
        toast("已删除「${ws.name}」")
        val remaining = guard("加载空间失败") { Api.listWorkspaces() } ?: emptyList()
        workspaces = remaining
        if (ws.id == workspace?.id) {
            val next = remaining.firstOrNull()
            if (next != null) {
                switchWorkspace(next)
            } else {
                workspace = null
                workspaces = emptyList()
                loggedIn = false
                goalSheet = false
                currentItem = null
                noteOpen = false
            }
        }
    }

    fun loadTree() {
        val ws = workspace ?: return
        val fresh = guard("加载目录失败") { Api.tree(ws.id) }
        if (fresh != null) {
            tree = fresh
            cacheScope.launch { CacheStore.put("tree:${ws.id}", fresh) }
            // 原型默认选中第一集作为刷题范围（而非「全部范围」）
            if (quizScopeId == null) {
                val firstEp = deriveCourses(fresh).firstOrNull()?.episodes?.firstOrNull()
                if (firstEp != null) loadQuiz(firstEp.nodeId, epScopeName(firstEp))
            }
        } else {
            cacheFallback<List<ItemNode>>(
                "tree:${ws.id}",
                empty = { tree.isEmpty() },
                apply = { tree = it },
                hint = "网络不可用 · 显示本地缓存的目录",
            )
        }
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
        if (itemId !in seenEpIds) {
            seenEpIds = seenEpIds + itemId
            prefs.edit()
                .putStringSet("seen_eps:${user?.id ?: workspace?.id}", seenEpIds.map { it.toString() }.toSet())
                .apply()
        }
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
        }
        if (pool != null) {
            quizPool = pool
            quizIdx = 0
            quizRight = 0
            quizWrong = 0
            quizAnswered = false
            quizOutcome = null
            quizPicked = -1
            cacheScope.launch { CacheStore.put("quiz:${ws.id}:${scopeId ?: "all"}", pool) }
        } else {
            cacheFallback<List<QuestionBrief>>(
                "quiz:${ws.id}:${scopeId ?: "all"}",
                empty = { quizPool.isEmpty() },
                apply = {
                    quizPool = it
                    quizIdx = 0
                    quizRight = 0
                    quizWrong = 0
                    quizAnswered = false
                    quizOutcome = null
                    quizPicked = -1
                },
                hint = "网络不可用 · 显示上次缓存的题目",
            )
        }
    }

    fun currentQuestion(): QuestionBrief? = quizPool.getOrNull(quizIdx)

    /** 选项索引 → wire chosen：single→数字、multi→索引数组、judge→布尔（0=错误 1=正确）。 */
    fun wireChosen(q: QuestionBrief, idx: Int): JsonElement = when (q.qtype) {
        QuestionType.Judge -> JsonPrimitive(idx == 1)
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

    /** 视频题作答：上传训练视频附件到题源笔记下 + 训练想法，不判分，提交待 AI 复盘。 */
    fun submitVideo(questionId: Long, sourceItemId: Long, files: List<Pair<String, ByteArray>>, note: String?) {
        val ids = files.mapNotNull { (name, bytes) ->
            guard("上传失败") { Api.uploadAttachment(sourceItemId, name, bytes) }
        }
        if (ids.isEmpty()) {
            toast("上传失败，请重试")
            return
        }
        val ok = guard("提交失败") {
            Api.videoAnswer(questionId, ids, note?.takeIf { it.isNotBlank() })
        } != null
        if (ok) {
            videoSubmitted = videoSubmitted + questionId
            toast("已提交，AI 复盘后会生成复盘笔记")
        }
    }

    // ==================== 错题本 ====================

    fun loadWrong() {
        val fresh = guard("加载错题失败") { Api.wrongList() }
        if (fresh != null) {
            wrongList = fresh
            cacheScope.launch { CacheStore.put("wrong:${user?.id ?: workspace?.id}", fresh) }
        } else {
            cacheFallback<List<WrongListItem>>(
                "wrong:${user?.id ?: workspace?.id}",
                empty = { wrongList.isEmpty() },
                apply = { wrongList = it },
                hint = "网络不可用 · 显示本地缓存的错题本",
            )
        }
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
        val ok = guard("操作失败") {
            if (next) Api.markMastered(item.wrong.questionId)
            else Api.unmarkMastered(item.wrong.questionId)
        } != null
        if (!ok) return
        if (next) {
            // 掌握后从列表移除（后端 list 只返回未掌握错题，本地同步移除，刷新也不再出现）
            wrongList = wrongList.filterIndexed { i, _ -> i != index }
            toast("已标记掌握")
        } else {
            val updated = wrongList.toMutableList()
            updated[index] = item.copy(wrong = item.wrong.copy(mastered = false))
            wrongList = updated
            toast("已取消掌握")
        }
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
        // P2-12：记录未交卷试卷 id，重新打开应用后经 GET /papers/:id 恢复会话。
        prefs.edit().putString("mock_paper_id", bundle.paper.id.toString()).apply()
    }

    /** 退出模考：清会话与恢复标记（进度不保留）。 */
    fun exitMock() {
        mockPaper = null
        prefs.edit().remove("mock_paper_id").apply()
    }

    /** P2-12：重新进入应用后恢复未交卷的模考（作答清空、计时重新开始）。 */
    fun restoreMock() {
        val id = prefs.getString("mock_paper_id", null)?.toLongOrNull() ?: return
        val bundle = guard("恢复模考失败") { Api.getPaper(id) } ?: run {
            prefs.edit().remove("mock_paper_id").apply()
            return
        }
        if (bundle.paper.result != null || bundle.questions.isEmpty()) {
            prefs.edit().remove("mock_paper_id").apply()
            return
        }
        startMock(bundle)
        toast("已恢复上次未交卷的模考")
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
        mockResult = result.result
        mockWrongQuestions = result.wrongQuestions
        loadWrong()
        mockPaper = null
        prefs.edit().remove("mock_paper_id").apply()
        onDone()
    }

    private fun toWireChosen(q: QuestionBrief, arr: JsonArray): JsonElement {
        val items = arr.map { it as JsonPrimitive }
        return when (q.qtype) {
            QuestionType.Multi -> JsonArray(items)
            QuestionType.Judge -> items.firstOrNull()?.let {
                JsonPrimitive(it.content.toIntOrNull() == 1)
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

    // ==================== 自定义 Skill ====================

    fun loadSkills() {
        val list = guard("获取 Skill 列表失败") { Api.listSkills() } ?: return
        skills = list
    }

    fun createSkill(): Boolean {
        if (skillName.isBlank() || skillDesc.isBlank()) {
            toast("请填写 Skill 名称与介绍")
            return false
        }
        val saved = guard("保存 Skill 失败") {
            Api.createSkill(NewSkillRequest(skillName.trim(), skillDesc.trim(), skillScript.ifBlank { null }))
        } ?: return false
        skills = skills + saved
        skillName = ""
        skillDesc = ""
        skillScript = ""
        toast("Skill「${saved.name}」已保存，接入的 Agent 下次拉取即可使用")
        return true
    }

    fun deleteSkill(id: Long) {
        val name = skills.firstOrNull { it.id == id }?.name ?: ""
        guard("删除 Skill 失败") {
            Api.deleteSkill(id)
            skills = skills.filterNot { it.id == id }
            Unit
        } ?: return
        toast("Skill「$name」已删除")
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

/** §12.5：AppState 托管于 ViewModel，折叠 / 分屏切换（配置变更重建 Activity）时状态不丢失。 */
class AppStateViewModel(application: Application) : AndroidViewModel(application) {
    val state = AppState(application)
}

@Composable
fun rememberAppState(): AppState =
    androidx.lifecycle.viewmodel.compose.viewModel<AppStateViewModel>().state
