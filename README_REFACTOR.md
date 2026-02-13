# DeepSeek API Client - Refactored Structure

This document describes the refactored module structure of the DeepSeek API client.

## Overview

The original monolithic `raw_request.rs` file has been refactored into a modular structure with clear separation of concerns. Each major struct and enum now resides in its own file, organized by logical grouping.

## Directory Structure

```
src/
├── lib.rs                    # Main library entry point
└── raw/                      # Raw API structures module
    ├── mod.rs               # Raw module exports
    ├── request/             # Request structures
    │   ├── mod.rs          # Request module exports
    │   ├── chat_completion.rs      # ChatCompletionRequest
    │   ├── message.rs              # Message, Role, ToolCall, ToolType, FunctionCall
    │   ├── model.rs                # Model enum
    │   ├── response_format.rs      # ResponseFormat, ResponseFormatType
    │   ├── stop.rs                 # Stop enum
    │   ├── stream_options.rs       # StreamOptions
    │   ├── thinking.rs             # Thinking, ThinkingType
    │   ├── tool.rs                 # Tool, Function
    │   └── tool_choice.rs          # ToolChoice, ToolChoiceType, ToolChoiceObject, FunctionName
    └── response/            # Response structures
        ├── mod.rs          # Response module exports
        ├── non_streaming/  # Non-streaming response structures
        │   ├── mod.rs      # Non-streaming exports
        │   ├── chat_completion_response.rs  # ChatCompletionResponse
        │   ├── choice.rs                     # Choice
        │   ├── finish_reason.rs              # FinishReason enum
        │   ├── logprobs.rs                   # Logprobs, TokenLogprob, TopLogprob
        │   ├── object_type.rs                # ObjectType enum
        │   └── usage.rs                      # Usage, CompletionTokensDetails
        └── streaming/      # Streaming response structures
            ├── mod.rs      # Streaming exports
            ├── chat_completion_chunk.rs      # ChatCompletionChunk
            ├── chunk_choice.rs               # ChunkChoice
            ├── chunk_object_type.rs          # ChunkObjectType
            └── delta.rs                      # Delta, DeltaToolCall, DeltaFunctionCall
```

## Key Improvements

### 1. **Modular Design**
   - Each struct/enum has its own file
   - Clear separation between request and response structures
   - Separate modules for streaming vs non-streaming responses

### 2. **Better Organization**
   - Related structures grouped together (e.g., all message-related structs in `message.rs`)
   - Logical hierarchy that mirrors API concepts

### 3. **Improved Maintainability**
   - Easier to locate and modify specific structures
   - Reduced file size and complexity
   - Clearer import relationships

### 4. **Enhanced Type Safety**
   - Proper module boundaries prevent accidental misuse
   - Clearer visibility of what's exported from each module

### 5. **API Completeness**
   - Added missing fields from API documentation:
     - `Message.prefix` field (Beta feature)
     - `Function.strict` field (Beta feature)
   - Proper serde attributes for all fields

## Usage Examples

### Basic Import
```rust
use ds_api::{
    ChatCompletionRequest, Message, Model, Role,
    ChatCompletionResponse, Choice, FinishReason,
};
```

### Creating a Request
```rust
let request = ChatCompletionRequest {
    messages: vec![Message {
        role: Role::User,
        content: Some("Hello, world!".to_string()),
        ..Default::default()
    }],
    model: Model::DeepseekChat,
    max_tokens: Some(100),
    temperature: Some(0.7),
    ..Default::default()
};
```

### With Tools
```rust
use ds_api::{Tool, ToolChoice, ToolChoiceType};
use serde_json::json;

let request = ChatCompletionRequest {
    messages: vec![/* ... */],
    tools: Some(vec![Tool {
        r#type: ds_api::ToolType::Function,
        function: ds_api::Function {
            name: "get_weather".to_string(),
            description: Some("Get weather for location".to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "location": {"type": "string"}
                },
                "required": ["location"]
            }),
            strict: Some(true),
        },
    }]),
    tool_choice: Some(ToolChoice::String(ToolChoiceType::Auto)),
    ..Default::default()
};
```

### Using DeepSeek Reasoner
```rust
use ds_api::{Thinking, ThinkingType};

let request = ChatCompletionRequest {
    messages: vec![/* ... */],
    model: Model::DeepseekReasoner,
    thinking: Some(Thinking {
        r#type: ThinkingType::Enabled,
    }),
    ..Default::default()
};
```

## Migration Guide

### From Old Structure
If you were using the old `raw_request.rs` file directly:

1. **Imports change**:
   ```rust
   // Old
   use ds_api::raw_request::{ChatCompletionRequest, Message, Role};
   
   // New
   use ds_api::{ChatCompletionRequest, Message, Role};
   ```

2. **Struct access remains the same**:
   - All struct and enum names are unchanged
   - Field names are unchanged
   - Serialization/deserialization behavior is unchanged

3. **New fields available**:
   - `Message.prefix: Option<bool>` - Beta feature for prefix continuation
   - `Function.strict: Option<bool>` - Beta feature for strict JSON schema validation

## Testing

Run the tests to verify the refactored structure:
```bash
cargo test
```

Run the example to see usage patterns:
```bash
cargo run --example basic_usage
```

## Design Decisions

1. **File-per-struct approach**: While some small related structs are grouped (e.g., `Message`, `Role`, `ToolCall` in `message.rs`), most major structs have their own file for clarity.

2. **Module hierarchy**: The hierarchy mirrors the API concepts:
   - `request/` vs `response/`
   - `non_streaming/` vs `streaming/` within responses

3. **Re-exports**: The library provides convenient re-exports at the crate root, so users don't need to navigate the module hierarchy.

4. **Backward compatibility**: All existing struct and enum names are preserved, and the public API remains compatible.

## Future Extensions

The modular structure makes it easy to:
1. Add new API endpoints as separate modules
2. Extend existing structures without touching unrelated code
3. Add validation logic to specific modules
4. Implement builder patterns for complex structs
5. Add helper functions and methods to appropriate modules

## See Also

- [DeepSeek API Documentation](https://api-docs.deepseek.com/zh-cn/api/create-chat-completion)
- `examples/basic_usage.rs` for comprehensive usage examples