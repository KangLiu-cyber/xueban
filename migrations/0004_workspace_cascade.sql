-- 删除备考空间：让 items / questions / papers 直接引用 workspaces 的外键级联。
-- 此前只支持删除目录/笔记（items 子树级联），删除空间会被这三张表的非级联外键阻断。
-- attachments 经 items(item_id) 级联；events 无外键、保留历史快照（同 item 删除语义）。

alter table items drop constraint items_workspace_id_fkey;
alter table items add constraint items_workspace_id_fkey
    foreign key (workspace_id) references workspaces on delete cascade;

alter table questions drop constraint questions_workspace_id_fkey;
alter table questions add constraint questions_workspace_id_fkey
    foreign key (workspace_id) references workspaces on delete cascade;

alter table papers drop constraint papers_workspace_id_fkey;
alter table papers add constraint papers_workspace_id_fkey
    foreign key (workspace_id) references workspaces on delete cascade;
