use uuid::Uuid;
use task_hookrs::*;
use serde_json;
use util::Result;
use std;
use std::collections::HashMap;
use std::process::Command;
use regex;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Task {
    pub status: status::TaskStatus,
    pub uuid: Uuid,
    pub entry: date::Date,
    pub description: String,

    pub partof: Option<Uuid>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Vec<annotation::Annotation>>,

    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub depends     : Option<String>,
    //
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<date::Date>,

    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub end         : Option<Date>,
    //
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub imask       : Option<i64>,
    //
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub mask        : Option<String>,
    //
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<date::Date>,

    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub parent      : Option<Uuid>,
    //
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub priority    : Option<TaskPriority>,
    //
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<project::Project>,

    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub recur       : Option<String>,
    //
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub scheduled   : Option<Date>,
    //
    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub start       : Option<Date>,
    //
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<tag::Tag>>,

    // #[serde(skip_serializing_if = "Option::is_none")]
    // pub until       : Option<Date>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<date::Date>,
}

pub struct TaskCache {
    tasks: HashMap<Uuid, Task>,
    children: HashMap<Uuid, Vec<Uuid>>,
}
impl TaskCache {
    pub fn new() -> Self {
        TaskCache {
            tasks: HashMap::new(),
            children: HashMap::new(),
        }
    }
    pub fn create(&mut self, description: &str, partof: Option<&Uuid>) -> Result<&Task> {
        lazy_static! {
            static ref UUID_RE: regex::Regex = regex::Regex::new("[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").unwrap();
        }
        let mut command = Command::new("task");
        command
            .stdout(std::process::Stdio::piped())
            .arg("add")
            .arg("rc.verbose=new-uuid")
            .arg(description);
        if let Some(uuid) = partof {
            command.arg(format!("partof:{}", uuid));
        };
        let stdout = command.output()?.stdout;
        let uuid_match = UUID_RE
            .captures_iter(std::str::from_utf8(&stdout)?)
            .next()
            .ok_or("No uuid in task feedback found")?;
        self.update(&Uuid::parse_str(&uuid_match[0])?)
    }
    pub fn refresh(&mut self) -> Result<()> {
        let stdout = &Command::new("task").arg("export").output()?.stdout;
        self.children.clear();
        for task in serde_json::from_str::<Vec<Task>>(std::str::from_utf8(stdout)?)? {
            if let Some(parent) = task.partof {
                if task.status != status::TaskStatus::Completed &&
                    task.status != status::TaskStatus::Deleted
                {
                    if self.children.contains_key(&parent) {
                        self.children.get_mut(&parent).unwrap().push(task.uuid);
                    } else {
                        self.children.insert(parent, vec![task.uuid]);
                    }
                }
            }
            self.tasks.insert(task.uuid, task);
        }
        Ok(())
    }
    pub fn get_task(&mut self, uuid: &Uuid) -> Result<&Task> {
        if cfg!(feature = "set_project") {
            let future_project = self.get_project_name(&uuid)?;
            if self.tasks
                .get(uuid)
                .ok_or("Uuid not found in Cache")?
                .project != future_project
            {
                if let Some(proj) = future_project {
                    project(uuid, Some(&proj))?;
                } else {
                    project(uuid, None)?;
                }
                self.update(uuid)?;
            }
        }
        if cfg!(feature = "set_project_tag") {
            let tag = "project".to_string();
            let has_tag = self.tasks
                .get(uuid)
                .ok_or("Uuid not found in Cache")?
                .tags
                .as_ref()
                .map_or(false, |vec| vec.contains(&tag));
            let needs_tag = self.children.contains_key(&uuid);
            if has_tag && !needs_tag {
                remove_tag(uuid, &tag)?;
            }
            if !has_tag && needs_tag {
                add_tag(uuid, &tag)?;
            }
            if has_tag != needs_tag {
                self.update(uuid)?;
            }
        }
        Ok(self.tasks.get(uuid).ok_or("Uuid not found in Cache")?)
    }
    pub fn update(&mut self, uuid: &Uuid) -> Result<&Task> {
        let stdout = &Command::new("task")
            .arg("export")
            .arg(format!("uuid:{}", uuid))
            .output()?
            .stdout;
        let task = serde_json::from_str::<Vec<Task>>(std::str::from_utf8(stdout)?)?
            .into_iter()
            .next()
            .ok_or("Could not load Task!")?;
        self.tasks.insert(*uuid, task);
        Ok(self.tasks.get(uuid).unwrap())
    }
    pub fn get_project_name(&self, uuid: &Uuid) -> Result<Option<String>> {
        let mut descriptions: Vec<&str> = Vec::new();
        let mut task = self.tasks.get(&uuid).ok_or("Uuid not found in Cache")?;
        while let Some(partof) = task.partof {
            task = self.tasks.get(&partof).ok_or("Uuid not found in Cache")?;
            descriptions.insert(0, &task.description);
        }
        if descriptions.len() > 0 {
            Ok(Some(descriptions.join(".").to_lowercase().replace(
                char::is_whitespace,
                "",
            )))
        } else {
            Ok(None)
        }
    }
}
pub fn get_tasks(query: &str) -> Result<Vec<Uuid>> {
    let stdout = &Command::new("task")
        .arg("_uuid")
        .arg(query)
        .output()?
        .stdout;
    let mut tasks = vec![];
    for uuid_str in std::str::from_utf8(stdout)?.split_whitespace() {
        tasks.push(Uuid::parse_str(uuid_str)?)
    }
    Ok(tasks)
}
pub fn project(uuid: &Uuid, project: Option<&str>) -> Result<()> {
    println!("Setting {}: project:{}", uuid, project.unwrap_or(""));
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("mod")
        .arg(format!("project:{}", project.unwrap_or("")))
        .output()?;
    Ok(())
}
pub fn done(uuid: &Uuid) -> Result<()> {
    println!("Setting {}: done", uuid);
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("done")
        .output()?;
    Ok(())
}
pub fn delete(uuid: &Uuid) -> Result<()> {
    println!("Setting {}: deleted", uuid);
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("delete")
        .arg("rc.confirmation:0")
        .output()?;
    Ok(())
}
pub fn pending(uuid: &Uuid) -> Result<()> {
    println!("Setting {}: pending", uuid);
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("mod")
        .arg("status:pending")
        .output()?;
    Ok(())
}
pub fn partof(uuid: &Uuid, partof: Option<&Uuid>) -> Result<()> {
    println!(
        "Setting {}: partof:{}",
        uuid,
        partof.map_or("".to_string(), ToString::to_string)
    );
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("mod")
        .arg(format!(
            "partof:{}",
            partof.map_or("".to_string(), ToString::to_string)
        ))
        .output()?;
    Ok(())
}
pub fn set_description(uuid: &Uuid, description: &str) -> Result<()> {
    println!("Setting {}: description:\"{}\"", uuid, description);
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("mod")
        .arg(format!("description:\"{}\"", description))
        .output()?;
    Ok(())
}
pub fn add_tag(uuid: &Uuid, tag: &str) -> Result<()> {
    println!("Setting {}: +{}", uuid, tag);
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("mod")
        .arg(format!("+{}", tag))
        .output()?;
    Ok(())
}
pub fn remove_tag(uuid: &Uuid, tag: &str) -> Result<()> {
    println!("Setting {}: -{}", uuid, tag);
    &Command::new("task")
        .arg(format!("uuid:{}", uuid))
        .arg("mod")
        .arg(format!("-{}", tag))
        .output()?;
    Ok(())
}
