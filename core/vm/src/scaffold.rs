use std::fs;
use std::path::Path;

pub fn execute_new(project_name: Option<String>, template: Option<String>, list_templates: bool) {
    if list_templates {
        println!("Available templates:\n");
        println!("  {:20} Minimal ECS hello-world", "default");
        println!(
            "  {:20} Approval pipeline with state machines, events, and SLA tracking",
            "workflow"
        );
        println!(
            "  {:20} Telemetry stream processor with windowed aggregation and alerting",
            "stream"
        );
        println!(
            "  {:20} Agent-based simulation with ECS entities, system ticks, and disruptions",
            "simulation"
        );
        println!(
            "  {:20} Service fleet management with autoscaling, health checks, and alerting",
            "control-plane"
        );
        println!("\nUsage: rad new <name> --template <template>");
        return;
    }

    let name = match project_name {
        Some(n) => n,
        None => {
            println!("Usage: rad new <project-name> [--template <template>]");
            println!("       rad new --list-templates");
            std::process::exit(1);
        }
    };

    let template_name = template.unwrap_or_else(|| "default".to_string());

    let (description, main_content) = match template_name.as_str() {
        "default" => (
            "Minimal ECS hello-world",
            include_str!("../../../tooling/templates/default.rad"),
        ),
        "workflow" => (
            "Approval pipeline with state machines, events, and SLA tracking",
            include_str!("../../../tooling/templates/workflow.rad"),
        ),
        "stream" => (
            "Telemetry stream processor with windowed aggregation and alerting",
            include_str!("../../../tooling/templates/stream.rad"),
        ),
        "simulation" => (
            "Agent-based simulation with ECS entities, system ticks, and disruptions",
            include_str!("../../../tooling/templates/simulation.rad"),
        ),
        "control-plane" => (
            "Service fleet management with autoscaling, health checks, and alerting",
            include_str!("../../../tooling/templates/control-plane.rad"),
        ),
        _ => {
            println!("Error: unknown template '{}'", template_name);
            println!("Available: default, workflow, stream, simulation, control-plane");
            println!("Run 'rad new --list-templates' for descriptions");
            std::process::exit(1);
        }
    };

    let path = Path::new(&name);
    if path.exists() {
        println!("Error: directory '{}' already exists", name);
        std::process::exit(1);
    }

    fs::create_dir_all(path.join("src")).unwrap();
    fs::create_dir_all(path.join("tests/snapshots")).unwrap();
    fs::create_dir_all(path.join("examples")).unwrap();

    let toml_content = format!(
        r#"[project]
name = "{}"
version = "0.1.0"
template = "{}"
description = "{}"

[build]
entry = "src/main.rad"
"#,
        name, template_name, description
    );
    fs::write(path.join("rad.toml"), toml_content).unwrap();

    let main_content = main_content.replace("__PROJECT_NAME__", &name);
    fs::write(path.join("src/main.rad"), main_content).unwrap();

    let test_basic_content = r#"// Basic tests for the project
fn test_arithmetic() {
  let result = 2 + 2
  print("2 + 2 =", result)
}

fn test_list_ops() {
  let items = [1, 2, 3]
  print("len =", len(items))
}
"#;
    fs::write(path.join("tests/test_basic.rad"), test_basic_content).unwrap();

    let snapshot_content = format!(
        "// Snapshot test — run `rad snapshot tests/snapshots` to verify\nprint(\"Hello from {}!\")\n",
        name
    );
    fs::write(path.join("tests/snapshots/hello.rad"), snapshot_content).unwrap();

    fs::write(path.join(".gitignore"), "build/\n*.radc\n").unwrap();

    println!("Created project '{}' (template: {})", name, template_name);
    println!("  {}/rad.toml", name);
    println!("  {}/src/main.rad", name);
    println!("  {}/tests/test_basic.rad", name);
    println!("  {}/tests/snapshots/hello.rad", name);
    println!("\nGet started:");
    println!("  cd {}", name);
    println!("  rad run");
    if template_name != "default" {
        println!("\nThis template includes a working {}.", description);
        println!("Edit src/main.rad to customize for your domain.");
    }
}
