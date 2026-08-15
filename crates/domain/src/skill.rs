//! Skill：系统内置 Skill 目录（文件资产）与用户自定义 Skill（持久化）。
//!
//! 内置 skill 放在仓库根 `skills/` 文件夹（一个子文件夹一个 skill，内含
//! `SKILL.md`），后端启动时加载为 Skill 目录，直接编辑文件即可更新；
//! 用户自定义 skill 存 skills 表（按用户隔离）。bootstrap 能力下发时两者
//! 合并（同名用户自定义覆盖内置），Agent 可按名经 get_skill 重新拉取。

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 内置 Skill：名称 + 介绍 + 脚本内容。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    /// 文字介绍：Agent 判断何时使用该 skill。
    pub description: String,
    /// 脚本内容（可空：纯文字说明型 skill 无脚本）。
    pub script: Option<String>,
}

/// 用户自定义 Skill：按用户隔离，存 skills 表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSkill {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub description: String,
    pub script: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 解析单个 skill 文件（`SKILL.md`）。
///
/// 可选 frontmatter（首行 `---` 至下一个 `---` 行，单行 `key: value` 字段：
/// `name` / `description`）；frontmatter 之后的正文即脚本。name 缺省取文件名主干，
/// description 缺省为空串；未知 frontmatter 字段忽略（宽容解析，开发者只管丢文件）。
pub fn parse_skill_file(stem: &str, content: &str) -> Result<Skill, String> {
    let content = content.trim_start_matches('\u{feff}');
    let mut name = stem.trim().to_owned();
    let mut description = String::new();
    let body = if let Some(rest) = content.strip_prefix("---") {
        let end = rest.find("\n---").ok_or("frontmatter 缺少结束行 `---`")?;
        for line in rest[..end].lines() {
            let line = line.trim();
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key.trim() {
                "name" => name = value.to_owned(),
                "description" => description = value.to_owned(),
                _ => {}
            }
        }
        rest[end + 4..].trim_start_matches('\n')
    } else {
        content
    };
    let script = body.trim();
    Ok(Skill {
        name,
        description,
        script: if script.is_empty() {
            None
        } else {
            Some(script.to_owned())
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let skill = parse_skill_file(
            "whatever.md",
            "---\nname: 链接转笔记\ndescription: 把视频链接整理成笔记\n---\n步骤1：解析链接\n步骤2：生成框架笔记\n",
        )
        .unwrap();
        assert_eq!(skill.name, "链接转笔记");
        assert_eq!(skill.description, "把视频链接整理成笔记");
        assert_eq!(
            skill.script.as_deref(),
            Some("步骤1：解析链接\n步骤2：生成框架笔记")
        );
    }

    #[test]
    fn name_falls_back_to_file_stem() {
        let skill = parse_skill_file(
            "习题生成",
            "---\ndescription: 基于笔记出题\n---\n生成 5 道单选题\n",
        )
        .unwrap();
        assert_eq!(skill.name, "习题生成");
        assert_eq!(skill.script.as_deref(), Some("生成 5 道单选题"));
    }

    #[test]
    fn no_frontmatter_means_whole_file_is_script() {
        let skill = parse_skill_file("链接转笔记", "直接把正文当作脚本\n第二行\n").unwrap();
        assert_eq!(skill.name, "链接转笔记");
        assert_eq!(skill.description, "");
        assert_eq!(skill.script.as_deref(), Some("直接把正文当作脚本\n第二行"));
    }

    #[test]
    fn empty_body_yields_no_script() {
        let skill =
            parse_skill_file("笔记", "---\nname: 笔记\ndescription: d\n---\n   \n").unwrap();
        assert_eq!(skill.name, "笔记");
        assert_eq!(skill.script, None);
    }

    #[test]
    fn unclosed_frontmatter_is_an_error() {
        assert!(parse_skill_file("坏文件", "---\nname: x\n正文").is_err());
    }
}
