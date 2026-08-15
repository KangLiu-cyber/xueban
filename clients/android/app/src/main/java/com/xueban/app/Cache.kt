package com.xueban.app

import android.content.Context
import androidx.room.ColumnInfo
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Entity
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.PrimaryKey
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json

/** §12.6：Room 只读缓存表（空间树 / 题目 / 错题本），payload 存 DTO 序列化 JSON。 */
@Entity(tableName = "cache_entries")
data class CacheEntry(
    @PrimaryKey @ColumnInfo(name = "cache_key") val key: String,
    val payload: String,
    @ColumnInfo(name = "updated_at") val updatedAt: Long,
)

@Dao
interface CacheDao {
    @Query("SELECT payload FROM cache_entries WHERE cache_key = :key")
    suspend fun get(key: String): String?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun upsert(entry: CacheEntry)
}

@Database(entities = [CacheEntry::class], version = 1, exportSchema = false)
abstract class XbCacheDb : RoomDatabase() {
    abstract fun cacheDao(): CacheDao
}

/** 只读缓存的读写入口：网络失败时降级读取上次成功的数据。 */
object CacheStore {
    @PublishedApi
    internal val json = Json {
        ignoreUnknownKeys = true
        coerceInputValues = true
    }

    @PublishedApi
    internal var db: XbCacheDb? = null

    fun init(context: Context) {
        if (db != null) return
        db = Room.databaseBuilder(context.applicationContext, XbCacheDb::class.java, "xueban-cache.db")
            .build()
    }

    suspend inline fun <reified T> put(key: String, value: T) {
        val payload = json.encodeToString(value)
        db?.cacheDao()?.upsert(CacheEntry(key, payload, System.currentTimeMillis()))
    }

    suspend inline fun <reified T> get(key: String): T? {
        val payload = db?.cacheDao()?.get(key) ?: return null
        return runCatching { json.decodeFromString<T>(payload) }.getOrNull()
    }
}
