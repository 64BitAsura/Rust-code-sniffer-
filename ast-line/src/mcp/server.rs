//! JSON-RPC 2.0 server loop for the MCP protocol over stdio.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        JsonRpcResponse { jsonrpc: "2.0".to_owned(), id, result: Some(result), error: None }
    }
    fn err(id: Option<Value>, code: i32, message: String) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_owned(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

pub fn run_mcp(index_dir: PathBuf) {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(req) => dispatch(req, &index_dir),
            Err(e) => JsonRpcResponse::err(None, -32700, format!("Parse error: {e}")),
        };

        let json = serde_json::to_string(&response).unwrap_or_default();
        let _ = writeln!(out, "{json}");
        let _ = out.flush();
    }
}

fn dispatch(req: JsonRpcRequest, index_dir: &PathBuf) -> JsonRpcResponse {
    let id = req.id.clone();
    let params = req.params.unwrap_or(Value::Null);

    match req.method.as_str() {
        "initialize" => JsonRpcResponse::ok(id, serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {}, "resources": {}, "prompts": {} },
            "serverInfo": { "name": "ast-line", "version": "0.1.0" }
        })),
        "tools/list" => JsonRpcResponse::ok(id, serde_json::json!({
            "tools": super::tools::list_tools()
        })),
        "tools/call" => {
            let tool_name = params["name"].as_str().unwrap_or("").to_owned();
            let tool_params = params["arguments"].clone();
            match super::tools::call_tool(&tool_name, tool_params, index_dir) {
                Ok(result) => JsonRpcResponse::ok(id, serde_json::json!({
                    "content": [{ "type": "text", "text": result }]
                })),
                Err(e) => JsonRpcResponse::err(id, -32000, e),
            }
        }
        "resources/list" => JsonRpcResponse::ok(id, serde_json::json!({
            "resources": [
                { "uri": "gitnexus://repo/context", "name": "Repository Context", "mimeType": "application/json" },
                { "uri": "gitnexus://repo/schema", "name": "Graph Schema", "mimeType": "application/json" }
            ]
        })),
        "resources/read" => {
            let uri = params["uri"].as_str().unwrap_or("").to_owned();
            let content = super::tools::read_resource(&uri, index_dir);
            JsonRpcResponse::ok(id, serde_json::json!({
                "contents": [{ "uri": uri, "text": content }]
            }))
        }
        "prompts/list" => JsonRpcResponse::ok(id, serde_json::json!({ "prompts": [] })),
        "prompts/get" => JsonRpcResponse::err(id, -32601, "Prompt not found".to_owned()),
        _ => JsonRpcResponse::err(id, -32601, format!("Method not found: {}", req.method)),
    }
}
