-- 笔记图片附件元数据：二进制落宿主磁盘（ATTACHMENTS_DIR/{user_id}/{uuid}），
-- 本表只存元数据。删除笔记时服务端先收集子树附件删磁盘文件，行随
-- item 级联删除兜底；崩溃窗口可能留下无表行引用的孤儿文件（无害，不做回收）。
create table attachments (
  id         bigserial primary key,
  item_id    bigint not null references items on delete cascade,
  filename   text not null,                -- 原始文件名（仅展示用）
  mime       text not null,                -- 以魔数嗅探结果为准，白名单 png/jpeg/gif/webp
  size_bytes bigint not null,
  uuid       text unique not null,         -- 磁盘文件名 {ATTACHMENTS_DIR}/{user_id}/{uuid}
  created_at timestamptz default now()
);

create index attachments_item_id_idx on attachments (item_id);
