//! 全局应用状态：登录态 / 学习空间树 / 进度集（localStorage 持久化）/
//! 视图路由 / 弹窗 / 刷题与组卷会话。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{NaiveDate, Timelike, Utc};
use gloo_storage::LocalStorage;
use gloo_storage::Storage as _;
use leptos::prelude::*;

use crate::api::{
    AnswerOutcome, Chosen, Item, ItemBundle, ItemKind, ItemNode, QuestionBrief, UserDto, Workspace,
    WrongListItem, WrongStats,
};

pub const LS_TOKEN: &str = "xb_token";
pub const LS_USER: &str = "xb_user";
pub const LS_WORKSPACE: &str = "xb_workspace";
pub const LS_STARS: &str = "xb_stars";
pub const LS_LEARNED: &str = "xb_learned";
pub const LS_PAPER_SEQ: &str = "xb_paper_seq";
/// 未交卷的模考试卷 id：页面刷新后经 GET /papers/:id 恢复会话。
pub const LS_MOCK_PAPER: &str = "xb_mock_paper";

pub fn ls_get(key: &str) -> Option<String> {
    LocalStorage::get(key).ok()
}

pub fn ls_set(key: &str, val: &str) {
    let _ = LocalStorage::set(key, val);
}

pub fn ls_remove(key: &str) {
    LocalStorage::delete(key);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Quiz,
    Wrong,
    Assembly,
    Notes,
    Mock,
}

#[derive(Clone, Debug)]
pub struct Episode {
    pub node_id: i64,
    pub title: String,
    pub notes: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct Course {
    pub dir_id: i64,
    pub name: String,
    pub subject: String,
    pub episodes: Vec<Episode>,
}

/// 课程名去「精讲/专项」等后缀，取 '·' 后部分作为学科标签；
/// 无 '·' 则取整个去后缀名。
fn subject_of(course_name: &str) -> String {
    let mut name = course_name.to_string();
    for suffix in ["精讲", "专项", "复习", "串讲"] {
        if name.ends_with(suffix) {
            name.truncate(name.len() - suffix.len());
            break;
        }
    }
    name.split('·')
        .map(|s| s.trim())
        .rfind(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| name.trim().to_string())
}

/// 从树推导课程 / 集（树结构：workspace → 课程 dir →（可折叠 folder dir）→ 集节点）。
/// 集节点 = 带笔记子节点的 Dir，或直接是 Note。编号在课程内重新编。
pub fn derive_courses(tree: &[ItemNode]) -> Vec<Course> {
    let mut courses = Vec::new();
    for node in tree {
        if node.item.kind != ItemKind::Dir {
            continue;
        }
        let mut episodes = Vec::new();
        collect_episodes(&node.children, &mut episodes);
        if episodes.is_empty() {
            continue;
        }
        courses.push(Course {
            dir_id: node.item.id,
            name: node.item.name.clone(),
            subject: subject_of(&node.item.name),
            episodes,
        });
    }
    courses
}

fn collect_episodes(children: &[ItemNode], out: &mut Vec<Episode>) {
    for child in children {
        match child.item.kind {
            ItemKind::Dir => {
                let has_note = child.children.iter().any(|c| c.item.kind == ItemKind::Note);
                if has_note {
                    let notes: Vec<Item> = child
                        .children
                        .iter()
                        .filter(|c| c.item.kind == ItemKind::Note)
                        .map(|c| c.item.clone())
                        .collect();
                    out.push(Episode {
                        node_id: child.item.id,
                        title: child.item.name.clone(),
                        notes,
                    });
                } else {
                    collect_episodes(&child.children, out);
                }
            }
            ItemKind::Note => out.push(Episode {
                node_id: child.item.id,
                title: child.item.name.clone(),
                notes: vec![child.item.clone()],
            }),
        }
    }
}

/// 集范围名称：单课程 '第{n}集 {title}'，多课程 '{course} 第{n}集 {title}'。
pub fn scope_name(courses: &[Course], c: usize, e: usize) -> String {
    let course = &courses[c];
    let ep = &course.episodes[e];
    if courses.len() == 1 {
        format!("第{}集 {}", e + 1, ep.title)
    } else {
        format!("{} 第{}集 {}", course.name, e + 1, ep.title)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum QuizScope {
    All,
    Episode(usize, usize),
    WrongOnly,
}

#[derive(Clone, Debug, PartialEq, Copy)]
pub enum WrongFilter {
    All,
    Comprehensive,
    Case,
    Starred,
    TwiceOrMore,
}

impl WrongFilter {
    pub fn label(&self) -> &'static str {
        match self {
            WrongFilter::All => "全部",
            WrongFilter::Comprehensive => "综合知识",
            WrongFilter::Case => "案例分析",
            WrongFilter::Starred => "★ 重点错题",
            WrongFilter::TwiceOrMore => "错 2 次以上",
        }
    }
}

/// 回调类型别名：保存回调等（Arc 便于跨闭包克隆，Send+Sync 供 spawn_local 使用）。
pub type CallbackFn = Arc<dyn Fn() + Send + Sync + 'static>;
pub type AnnoSaveFn = Arc<dyn Fn(String) + Send + Sync + 'static>;
pub type ReloadFn = Arc<dyn Fn() + Send + Sync + 'static>;

#[derive(Clone)]
pub struct ConfirmSpec {
    pub title: String,
    pub text_html: String,
    pub ok_label: String,
    pub on_ok: CallbackFn,
}

#[derive(Clone)]
pub struct MockSession {
    pub paper_id: i64,
    pub name: String,
    pub questions: Vec<QuestionBrief>,
    pub answers: Vec<Option<Chosen>>,
    pub idx: usize,
    pub marked: HashSet<usize>,
    pub start_ms: f64,
    pub remaining: u32,
}

#[derive(Clone)]
pub struct MockResult {
    pub score: u32,
    pub correct: u32,
    pub total: u32,
    pub duration_secs: u32,
    pub wrong_count: u32,
    pub skip_count: u32,
}

#[derive(Clone)]
pub struct RedoState {
    pub outcome: Option<AnswerOutcome>,
    pub picked: Option<Chosen>,
    pub mastered: bool,
}

#[derive(Clone, Copy)]
pub struct AppState {
    /// 是否已进入主界面。登录成功后先停留在登录页第二步（创建备考空间），
    /// 确认创建（或已有空间）后才置 true 切换进 Shell —— 与 token 写入解耦。
    pub entered: RwSignal<bool>,
    pub token: RwSignal<Option<String>>,
    pub user: RwSignal<Option<UserDto>>,
    pub workspaces: RwSignal<Vec<Workspace>>,
    pub workspace: RwSignal<Option<Workspace>>,
    pub tree: RwSignal<Vec<ItemNode>>,
    pub courses: RwSignal<Vec<Course>>,
    pub learned: RwSignal<HashSet<i64>>,
    pub stars: RwSignal<HashSet<i64>>,
    pub paper_seq: RwSignal<u32>,
    pub view: RwSignal<View>,
    pub episode: RwSignal<Option<(usize, usize)>>,
    pub note_bundles: RwSignal<Vec<ItemBundle>>,
    pub toast: RwSignal<Option<(String, u64)>>,
    pub setup_open: RwSignal<bool>,
    pub agent_open: RwSignal<bool>,
    pub anno_open: RwSignal<bool>,
    pub anno_detail: RwSignal<Option<AnnoDetail>>,
    pub confirm: RwSignal<Option<ConfirmSpec>>,
    pub preview_open: RwSignal<bool>,
    pub topbar_override: RwSignal<Option<(String, String)>>,
    pub fab_pos: RwSignal<Option<(f64, f64)>>,
    pub anno_quote: RwSignal<String>,
    pub anno_text: RwSignal<String>,
    pub anno_save: RwSignal<Option<AnnoSaveFn>>,
    pub note_reload: RwSignal<Option<ReloadFn>>,
    pub preview: RwSignal<Option<PreviewPaper>>,
    pub agent_text: RwSignal<String>,
    pub redo_open: RwSignal<bool>,
    pub mock_result: RwSignal<Option<MockResult>>,
    pub quiz_scope: RwSignal<QuizScope>,
    pub quiz_pool: RwSignal<Vec<QuestionBrief>>,
    pub quiz_answers: RwSignal<Vec<Option<Chosen>>>,
    pub quiz_outcomes: RwSignal<Vec<Option<AnswerOutcome>>>,
    pub quiz_chosen: RwSignal<Vec<Option<Chosen>>>,
    pub quiz_idx: RwSignal<usize>,
    pub quiz_correct_cnt: RwSignal<u32>,
    pub quiz_wrong_cnt: RwSignal<u32>,
    pub quiz_badge: RwSignal<u32>,
    pub wrong_list: RwSignal<Vec<WrongListItem>>,
    pub wrong_stats: RwSignal<Option<WrongStats>>,
    pub wrong_filter: RwSignal<WrongFilter>,
    pub wrong_badge: RwSignal<u32>,
    pub redo_list: RwSignal<Vec<WrongListItem>>,
    pub redo_idx: RwSignal<usize>,
    pub redo_state: RwSignal<Vec<RedoState>>,
    pub pool: RwSignal<Vec<QuestionBrief>>,
    pub selected: RwSignal<HashSet<i64>>,
    pub paper_name: RwSignal<String>,
    pub mock: RwSignal<Option<MockSession>>,
}

/// 批注详情弹窗数据。
#[derive(Clone)]
pub struct AnnoDetail {
    pub quote: String,
    pub text: String,
    pub mine: bool,
    pub item_id: i64,
    pub anno_id: i64,
}

/// 组卷预览数据。
#[derive(Clone)]
pub struct PreviewPaper {
    pub name: String,
    pub questions: Vec<QuestionBrief>,
    pub total: u32,
    pub score: u32,
    pub duration_secs: u32,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        let token = ls_get(LS_TOKEN);
        crate::api::set_auth_token(token.clone());
        let user = ls_get(LS_USER).and_then(|s| serde_json::from_str::<UserDto>(&s).ok());
        let workspace =
            ls_get(LS_WORKSPACE).and_then(|s| serde_json::from_str::<Workspace>(&s).ok());
        let stars = ls_get(LS_STARS)
            .and_then(|s| serde_json::from_str::<Vec<i64>>(&s).ok())
            .map(|v| v.into_iter().collect::<HashSet<i64>>())
            .unwrap_or_default();
        let learned = ls_get(LS_LEARNED)
            .and_then(|s| serde_json::from_str::<Vec<i64>>(&s).ok())
            .map(|v| v.into_iter().collect::<HashSet<i64>>())
            .unwrap_or_default();
        let paper_seq = ls_get(LS_PAPER_SEQ)
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);

        Self {
            entered: RwSignal::new(ls_get(LS_TOKEN).is_some()),
            token: RwSignal::new(token),
            user: RwSignal::new(user),
            workspaces: RwSignal::new(Vec::new()),
            workspace: RwSignal::new(workspace),
            tree: RwSignal::new(Vec::new()),
            courses: RwSignal::new(Vec::new()),
            learned: RwSignal::new(learned),
            stars: RwSignal::new(stars),
            paper_seq: RwSignal::new(paper_seq),
            view: RwSignal::new(View::Notes),
            episode: RwSignal::new(None),
            note_bundles: RwSignal::new(Vec::new()),
            toast: RwSignal::new(None),
            setup_open: RwSignal::new(false),
            agent_open: RwSignal::new(false),
            anno_open: RwSignal::new(false),
            anno_detail: RwSignal::new(None),
            confirm: RwSignal::new(None),
            preview_open: RwSignal::new(false),
            topbar_override: RwSignal::new(None),
            fab_pos: RwSignal::new(None),
            anno_quote: RwSignal::new(String::new()),
            anno_text: RwSignal::new(String::new()),
            anno_save: RwSignal::new(None::<AnnoSaveFn>),
            note_reload: RwSignal::new(None::<ReloadFn>),
            preview: RwSignal::new(None),
            agent_text: RwSignal::new(String::new()),
            redo_open: RwSignal::new(false),
            mock_result: RwSignal::new(None),
            quiz_scope: RwSignal::new(QuizScope::All),
            quiz_pool: RwSignal::new(Vec::new()),
            quiz_answers: RwSignal::new(Vec::new()),
            quiz_outcomes: RwSignal::new(Vec::new()),
            quiz_chosen: RwSignal::new(Vec::new()),
            quiz_idx: RwSignal::new(0),
            quiz_correct_cnt: RwSignal::new(0),
            quiz_wrong_cnt: RwSignal::new(0),
            quiz_badge: RwSignal::new(0),
            wrong_list: RwSignal::new(Vec::new()),
            wrong_stats: RwSignal::new(None),
            wrong_filter: RwSignal::new(WrongFilter::All),
            wrong_badge: RwSignal::new(0),
            redo_list: RwSignal::new(Vec::new()),
            redo_idx: RwSignal::new(0),
            redo_state: RwSignal::new(Vec::new()),
            pool: RwSignal::new(Vec::new()),
            selected: RwSignal::new(HashSet::new()),
            paper_name: RwSignal::new(String::new()),
            mock: RwSignal::new(None),
        }
    }

    /// 只写凭证与 token 信号（登录流程中第二步创建空间前不置 user，
    /// 避免 App 提前切换到主界面）。
    pub fn persist_creds(&self, token: &str, user: &UserDto) {
        ls_set(LS_TOKEN, token);
        ls_set(LS_USER, &serde_json::to_string(user).unwrap_or_default());
        crate::api::set_auth_token(Some(token.to_string()));
        self.token.set(Some(token.to_string()));
    }

    pub fn persist_user(&self, token: &str, user: &UserDto) {
        self.persist_creds(token, user);
        self.user.set(Some(user.clone()));
    }

    pub fn clear_auth(&self) {
        ls_remove(LS_TOKEN);
        ls_remove(LS_USER);
        ls_remove(LS_WORKSPACE);
        crate::api::set_auth_token(None);
        self.entered.set(false);
        self.token.set(None);
        self.user.set(None);
        self.workspace.set(None);
        self.workspaces.set(Vec::new());
        self.tree.set(Vec::new());
        self.courses.set(Vec::new());
        self.view.set(View::Notes);
        self.episode.set(None);
    }

    pub fn set_workspace(&self, ws: &Workspace) {
        ls_set(LS_WORKSPACE, &serde_json::to_string(ws).unwrap_or_default());
        self.workspace.set(Some(ws.clone()));
    }

    pub fn toggle_star(&self, question_id: i64) -> bool {
        let mut stars = self.stars.get_untracked();
        let on = if stars.contains(&question_id) {
            stars.remove(&question_id);
            false
        } else {
            stars.insert(question_id);
            true
        };
        self.stars.set(stars.clone());
        let v: Vec<i64> = stars.into_iter().collect();
        ls_set(LS_STARS, &serde_json::to_string(&v).unwrap_or_default());
        on
    }

    pub fn is_starred(&self, question_id: i64) -> bool {
        self.stars.get_untracked().contains(&question_id)
    }

    pub fn mark_learned(&self, node_id: i64) {
        let mut set = self.learned.get_untracked();
        if set.insert(node_id) {
            self.learned.set(set.clone());
            let v: Vec<i64> = set.into_iter().collect();
            ls_set(LS_LEARNED, &serde_json::to_string(&v).unwrap_or_default());
        }
    }

    pub fn next_paper_seq(&self) -> u32 {
        let n = self.paper_seq.get_untracked() + 1;
        self.paper_seq.set(n);
        ls_set(LS_PAPER_SEQ, &n.to_string());
        n
    }

    pub fn toast(&self, msg: &str) {
        let nonce = self.toast.get_untracked().map(|(_, n)| n + 1).unwrap_or(1);
        self.toast.set(Some((msg.to_string(), nonce)));
    }

    /// 课程集的总笔记节点数（「已学 X / N 集」用）。
    pub fn course_progress(&self, c: usize) -> (usize, usize) {
        let courses = self.courses.get_untracked();
        if let Some(course) = courses.get(c) {
            let total = course.episodes.len();
            let learned = course
                .episodes
                .iter()
                .filter(|e| self.learned.get_untracked().contains(&e.node_id))
                .count();
            (learned, total)
        } else {
            (0, 0)
        }
    }
}

/// 考试目标时间：考试日 09:00，已过则返回 0。
pub fn exam_days_left(exam_date: Option<NaiveDate>) -> i64 {
    let Some(d) = exam_date else {
        return 0;
    };
    let now = Utc::now();
    let today = now.date_naive();
    if d < today {
        return 0;
    }
    let days = (d - today).num_days();
    // 考试日当天按 09:00 是否已过判断
    if days == 0 {
        let hour = now.hour() as i64;
        if hour >= 9 {
            return 0;
        }
    }
    days.max(0)
}

pub fn fmt_date_ymd(d: chrono::DateTime<Utc>) -> String {
    d.date_naive().format("%Y-%m-%d").to_string()
}

pub fn fmt_duration(secs: u32) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// 答题时长的 mock 参考值：count × 120s。
pub fn mock_duration_secs(count: u32) -> u32 {
    count * 120
}

/// 集节点 id（含集内笔记 id）→ (课程下标, 集下标)，供来源标签 / 树反查。
pub fn episode_map(courses: &[Course]) -> HashMap<i64, (usize, usize)> {
    let mut m = HashMap::new();
    for (c, course) in courses.iter().enumerate() {
        for (e, ep) in course.episodes.iter().enumerate() {
            m.insert(ep.node_id, (c, e));
            for note in &ep.notes {
                m.insert(note.id, (c, e));
            }
        }
    }
    m
}

/// 子树内的集节点数（课程/文件夹行的「N 集」角标）。
pub fn count_episodes(node: &ItemNode) -> usize {
    if node.item.kind == ItemKind::Note {
        return 1;
    }
    let has_note = node.children.iter().any(|c| c.item.kind == ItemKind::Note);
    if has_note {
        1
    } else {
        node.children.iter().map(count_episodes).sum()
    }
}
