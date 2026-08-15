package com.xueban.app

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import java.util.concurrent.TimeUnit

/**
 * /api/v1 类型化客户端。DTO 与 crates/adapter-http 各 handler 的 wire shape 一一对应
 * （docs/requirements.md §API）；作答 chosen 为 untagged 线格式
 * （single→数字、multi→索引数组、judge→布尔）。
 */
object Api {
    // Android 模拟器经 10.0.2.2 访问宿主机后端；真机请改为局域网地址。
    const val BASE_URL = "http://10.0.2.2:8080/api/v1"

    val json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
    }

    private val client = OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(20, TimeUnit.SECONDS)
        .build()

    private val mediaType = "application/json; charset=utf-8".toMediaType()

    @Volatile
    var authToken: String? = null

    class ApiException(val code: Int, message: String) : Exception(message)

    private fun exec(method: String, path: String, body: String? = null): String {
        val url = BASE_URL + path
        val b = Request.Builder().url(url)
        b.header("Content-Type", "application/json")
        authToken?.let { b.header("Authorization", "Bearer $it") }
        val rb = body?.toRequestBody(mediaType)
        val req = when (method) {
            "POST" -> b.post(rb ?: "{}".toRequestBody(mediaType)).build()
            "PUT" -> b.put(rb ?: "{}".toRequestBody(mediaType)).build()
            "DELETE" -> b.delete().build()
            else -> b.get().build()
        }
        client.newCall(req).execute().use { resp ->
            val text = resp.body?.string() ?: ""
            if (resp.code in 200..299) return text
            if (resp.code == 401) authToken = null
            val msg = runCatching {
                json.decodeFromString<JsonObject>(text)["error"]?.toString()?.trim('"')
            }.getOrNull() ?: text
            throw ApiException(resp.code, msg)
        }
    }

    private inline fun <reified T> get(path: String): T =
        json.decodeFromString(exec("GET", path))

    private inline fun <reified T, reified B> post(path: String, body: B): T =
        json.decodeFromString(exec("POST", path, json.encodeToString(body)))

    private inline fun <reified T, reified B> put(path: String, body: B): T =
        json.decodeFromString(exec("PUT", path, json.encodeToString(body)))

    private fun postVoid(path: String) {
        exec("POST", path)
    }

    private fun deleteVoid(path: String) {
        exec("DELETE", path)
    }

    // ---- 身份 ----

    fun register(account: String, password: String, nickname: String? = null): AuthResponse =
        post("/auth/register", RegisterRequest(account, password, nickname))

    fun login(account: String, password: String): AuthResponse =
        post("/auth/login", LoginRequest(account, password))

    fun logout() {
        postVoid("/auth/logout")
    }

    // ---- 空间 ----

    fun listWorkspaces(): List<Workspace> = get("/workspaces")

    fun createWorkspace(input: WorkspaceInput): Workspace =
        post("/workspaces", input)

    fun updateWorkspace(id: Long, input: WorkspaceInput): Workspace =
        put("/workspaces/$id", input)

    fun tree(workspaceId: Long): List<ItemNode> = get("/workspaces/$workspaceId/tree")

    fun itemBundle(itemId: Long): ItemBundle = get("/items/$itemId")

    fun addAnnotation(itemId: Long, input: AnnotationInput): Annotation =
        post("/items/$itemId/annotations", input)

    fun deleteAnnotation(id: Long) {
        deleteVoid("/annotations/$id")
    }

    // ---- 刷题 ----

    fun draw(workspaceId: Long, scope: Long? = null, count: Int = 10): List<QuestionBrief> {
        val qs = mutableListOf("workspace_id=$workspaceId")
        scope?.let { qs += "scope=$it" }
        qs += "count=$count"
        return get("/quiz/questions?${qs.joinToString("&")}")
    }

    fun answer(request: AnswerRequest): AnswerOutcome =
        post("/quiz/answer", request)

    // ---- 错题本 ----

    fun wrongList(): List<WrongListItem> = get("/wrong")

    fun wrongStats(): WrongStats = get("/wrong/stats")

    fun markMastered(questionId: Long) {
        postVoid("/wrong/$questionId/master")
    }

    fun unmarkMastered(questionId: Long) {
        postVoid("/wrong/$questionId/unmaster")
    }

    // ---- 组卷 ----

    fun assemblePaper(request: AssembleRequest): PaperBundle =
        post("/papers", request)

    fun submitPaper(paperId: Long, request: SubmitRequest): PaperResult =
        post("/papers/$paperId/submit", request)

    // ---- Agent 凭证 ----

    fun credential(): CredentialResponse = get("/agent/credential")

    fun rotateCredential(): CredentialResponse = post("/agent/credential/rotate", JsonObject(emptyMap()))
}

// ==================== DTO（字段名与后端 serde 默认 snake_case 输出一致） ====================

@Serializable
data class RegisterRequest(
    val account: String,
    val password: String,
    val nickname: String? = null,
)

@Serializable
data class LoginRequest(
    val account: String,
    val password: String,
)

@Serializable
data class UserDto(
    val id: Long,
    val account: String,
    val nickname: String? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class AuthResponse(
    val token: String,
    val user: UserDto,
)

@Serializable
data class WorkspaceInput(
    val name: String,
    @SerialName("exam_goal") val examGoal: String,
    @SerialName("exam_date") val examDate: String? = null,
)

@Serializable
data class Workspace(
    val id: Long,
    @SerialName("user_id") val userId: Long,
    val name: String,
    @SerialName("exam_goal") val examGoal: String,
    @SerialName("exam_date") val examDate: String? = null,
    @SerialName("created_at") val createdAt: String,
    // 后端 workspaces 表无 updated_at 列，必填会致反序列化失败。
    @SerialName("updated_at") val updatedAt: String? = null,
)

@Serializable
enum class ItemKind { @SerialName("dir") Dir, @SerialName("note") Note }

@Serializable
enum class Creator { @SerialName("agent") Agent, @SerialName("user") User }

@Serializable
data class Item(
    val id: Long,
    @SerialName("workspace_id") val workspaceId: Long,
    @SerialName("parent_id") val parentId: Long? = null,
    val kind: ItemKind,
    val name: String,
    val content: String? = null,
    @SerialName("created_by") val createdBy: Creator,
    @SerialName("created_at") val createdAt: String,
    @SerialName("updated_at") val updatedAt: String,
)

@Serializable
data class ItemNode(
    val item: Item,
    val children: List<ItemNode> = emptyList(),
)

@Serializable
enum class AnnotationAuthor { @SerialName("user") User, @SerialName("ai") Ai }

@Serializable
data class Annotation(
    val id: Long,
    @SerialName("item_id") val itemId: Long,
    @SerialName("user_id") val userId: Long,
    val author: AnnotationAuthor,
    val anchor: String,
    val text: String,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class AnnotationInput(
    val anchor: String,
    val text: String,
)

@Serializable
data class ItemBundle(
    val item: Item,
    val annotations: List<Annotation> = emptyList(),
)

@Serializable
enum class QuestionType {
    @SerialName("single") Single,
    @SerialName("multi") Multi,
    @SerialName("judge") Judge,
}

@Serializable
data class QuestionBrief(
    val id: Long,
    @SerialName("source_item_id") val sourceItemId: Long,
    val qtype: QuestionType,
    val stem: String,
    val options: List<String> = emptyList(),
)

/// 作答 chosen 的 untagged 线格式：single→数字、multi→索引数组、judge→布尔。
@Serializable
data class AnswerRequest(
    @SerialName("question_id") val questionId: Long,
    val chosen: JsonElement,
    val scope: String? = null,
)

@Serializable
data class AnswerOutcome(
    @SerialName("is_correct") val isCorrect: Boolean,
    val answer: JsonElement,
    val explanation: String? = null,
)

@Serializable
data class WrongItem(
    val id: Long,
    @SerialName("user_id") val userId: Long,
    @SerialName("question_id") val questionId: Long,
    val times: Int = 0,
    val mastered: Boolean = false,
    @SerialName("updated_at") val updatedAt: String,
)

@Serializable
data class WrongListItem(
    val wrong: WrongItem,
    val question: QuestionBrief,
)

@Serializable
data class WrongStats(
    val total: Int = 0,
    @SerialName("weekly_new") val weeklyNew: Int = 0,
    val mastered: Int = 0,
)

@Serializable
data class PaperConfig(
    val scope: String? = null,
    @SerialName("question_types") val questionTypes: List<QuestionType>? = null,
    @SerialName("source_item_ids") val sourceItemIds: List<Long>? = null,
    val count: Int = 75,
)

@Serializable
data class AssembleRequest(
    @SerialName("workspace_id") val workspaceId: Long,
    val name: String? = null,
    val config: PaperConfig,
)

@Serializable
data class PaperResult(
    val score: Int = 0,
    val correct: Int = 0,
    val total: Int = 0,
    @SerialName("duration_secs") val durationSecs: Int = 0,
)

@Serializable
data class Paper(
    val id: Long,
    @SerialName("user_id") val userId: Long,
    @SerialName("workspace_id") val workspaceId: Long,
    // 后端 name 为 Option，未命名组卷时返回 null。
    val name: String? = null,
    val config: PaperConfig,
    val result: PaperResult? = null,
    @SerialName("created_at") val createdAt: String,
)

@Serializable
data class PaperBundle(
    val paper: Paper,
    val questions: List<QuestionBrief> = emptyList(),
)

@Serializable
data class PaperAnswer(
    @SerialName("question_id") val questionId: Long,
    val chosen: JsonElement,
)

@Serializable
data class SubmitRequest(
    val answers: List<PaperAnswer>,
    @SerialName("duration_secs") val durationSecs: Int,
)

@Serializable
data class CredentialResponse(
    val token: String,
    val endpoint: String,
)
