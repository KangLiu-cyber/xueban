-- Item 删除级联（P2-14）：删除目录/笔记时级联清理子树、批注与归属习题，
-- 习题删除进一步级联作答记录与错题；events.item_id 无外键，保留历史快照。

-- 子树：目录删除级联子节点（子节点继续级联各自的子树/批注/习题）。
alter table items
  drop constraint items_parent_id_fkey,
  add constraint items_parent_id_fkey foreign key (parent_id) references items on delete cascade;

-- 批注归属内容。
alter table annotations
  drop constraint annotations_item_id_fkey,
  add constraint annotations_item_id_fkey foreign key (item_id) references items on delete cascade;

-- 习题归属内容（"集"）：内容删除级联其题库。
alter table questions
  drop constraint questions_source_item_id_fkey,
  add constraint questions_source_item_id_fkey foreign key (source_item_id) references items on delete cascade;

-- 作答记录与错题归属习题：习题删除级联清理。
alter table quiz_records
  drop constraint quiz_records_question_id_fkey,
  add constraint quiz_records_question_id_fkey foreign key (question_id) references questions on delete cascade;

alter table wrong_items
  drop constraint wrong_items_question_id_fkey,
  add constraint wrong_items_question_id_fkey foreign key (question_id) references questions on delete cascade;
