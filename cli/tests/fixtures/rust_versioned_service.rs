use axum::{Extension, Json, http::StatusCode};
use forklaunch_axum::ForkLaunchRouter;
use forklaunch_contract::{BodyContract, BodyKind, RequestContract, ResponseContract, RouteContract, VersionedContract};
use forklaunch_http::SelectedVersion;
use serde_json::{Value,json};

pub fn contracts() -> Vec<RouteContract> {
    let mut route=RouteContract::post("Write","Write a versioned value","/versions");
    for (version,kind,status) in [("v1","string",201),("v2","integer",202)] {
        let mut input=BodyContract::json("VersionInput");
        input.schema=input.schema.with_json_schema(json!({"type":"object","additionalProperties":false,"required":["kind","value"],"properties":{"kind":{"type":"string","const":version},"value":{"type":kind}}}).to_string());
        let mut output=BodyContract::json("VersionOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"type":kind}}}).to_string());
        route=route.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(status,Some(output))]});
    }
    let mut text=RouteContract::post("Write Text","Write versioned text","/versioned-text");
    text.options.body_parser.text_wire.limit_bytes=8;
    for (version,minimum,status) in [("v1",0,201),("v2",2,202)] {
        let input=BodyContract { kind:BodyKind::Text, content_type:Some("text/plain".into()), ..BodyContract::json("TextInput") };
        let mut input=input;
        input.schema=input.schema.with_json_schema(json!({"type":"string","minLength":minimum}).to_string());
        let mut output=BodyContract::json("TextOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","required":["version","value"],"properties":{"version":{"type":"string"},"value":{"type":"string"}}}).to_string());
        text=text.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(status,Some(output))]});
    }
    let mut file=RouteContract::post("Write File","Write versioned file","/versioned-file");
    for version in ["v1","v2"] {
        let mut input=BodyContract { kind:BodyKind::File, content_type:Some("application/octet-stream".into()), ..BodyContract::json("FileInput") };
        input.schema=input.schema.with_json_schema(json!({"type":"string","format":"binary"}).to_string());
        let mut output=BodyContract::json("FileOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","required":["version","bytes"],"properties":{"version":{"type":"string"},"bytes":{"type":"array","items":{"type":"integer"}}}}).to_string());
        file=file.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(202,Some(output))]});
    }
    let mut form=RouteContract::post("Write Form","Write versioned form","/versioned-form");
    form.options.body_parser.form_charset_sentinel=true;
    form.options.body_parser.form_interpret_numeric_entities=true;
    for (version,minimum,status) in [("v1",1,201),("v2",2,202),("v3",0,200)] {
        let mut input=BodyContract { kind:BodyKind::UrlEncoded, content_type:Some("application/x-www-form-urlencoded".into()), ..BodyContract::json("FormInput") };
        let value=if version=="v3" {json!({"type":"object","required":["items"],"properties":{"items":{"type":"array","minItems":2,"items":{"type":"string"}}}})} else {json!({"type":"string","minLength":minimum})};
        input.schema=input.schema.with_json_schema(json!({"type":"object","required":["value"],"properties":{"value":value}}).to_string());
        let mut output=BodyContract::json("FormOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","required":["version","raw"],"properties":{"version":{"type":"string"},"raw":{"type":"string"}}}).to_string());
        form=form.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(status,Some(output))]});
    }
    let mut multipart=RouteContract::post("Write Multipart","Write versioned multipart","/versioned-multipart");
    for (version,minimum,status) in [("v1",1,201),("v2",2,202)] {
        let mut input=BodyContract { kind:BodyKind::Multipart, content_type:Some("multipart/form-data".into()), ..BodyContract::json("MultipartInput") };
        input.schema=input.schema.with_json_schema(json!({"$schema":"https://json-schema.org/draft/2020-12/schema","$id":"https://example.invalid/multipart","$defs":{"File":{"$id":"attachment","$anchor":"bytes","type":"string","format":"binary"}},"type":"object","required":["label"],"properties":{"label":{"type":"string","minLength":minimum},"file":{"$ref":"attachment#bytes"}}}).to_string());
        let mut output=BodyContract::json("MultipartOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","required":["version","bytes"],"properties":{"version":{"type":"string"},"bytes":{"type":"array","items":{"type":"integer"}}}}).to_string());
        multipart=multipart.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(status,Some(output))]});
    }
    let mut scalar=RouteContract::post("Write Scalar","Write scalar JSON","/versioned-scalar");
    scalar.options.body_parser.json_strict=false;
    scalar.options.body_parser.json_wire.inflate=false;
    for (version,kind,status) in [("v1","integer",201),("v2","string",202)] {
        let mut input=BodyContract::json("ScalarInput");
        input.schema=input.schema.with_json_schema(json!({"type":kind}).to_string());
        let mut output=BodyContract::json("ScalarOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","required":["value"],"properties":{"value":{"type":kind}}}).to_string());
        scalar=scalar.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(status,Some(output))]});
    }
    let mut transformed=RouteContract::post("Write Transform","Transform versioned JSON","/versioned-transform");
    for (version,minimum,status) in [("v1",1,201),("v2",2,202)] {
        let mut input=BodyContract::json("TransformInput");
        input.schema=input.schema.with_json_schema(json!({"type":"object","required":["value"],"properties":{"value":{"type":"string","minLength":minimum}}}).to_string());
        let mut output=BodyContract::json("TransformOutput");
        output.schema=output.schema.with_json_schema(json!({"type":"object","required":["value"],"properties":{"value":{"type":"string"}}}).to_string());
        transformed=transformed.with_version(VersionedContract{version:version.into(),request:RequestContract::default().with_body(input),responses:vec![ResponseContract::new(status,Some(output))]});
    }
    vec![route,text,file,form,multipart,scalar,transformed]
}
pub fn register(router:ForkLaunchRouter) -> ForkLaunchRouter {
    let mut hooks=forklaunch_http::VersionBodyHooksRegistry::default();
    hooks.insert_parser(&contracts()[6],forklaunch_http::BodyParserHooks::default().with_verify(|request,bytes,encoding| {
        if encoding!=Some("utf-8") { return Err("unexpected encoding".into()); }
        if request.headers.get("x-reject-body").map(String::as_str)==Some("yes") || bytes.windows(9).any(|window|window==b"wire-deny") {
            return Err("private verification detail".into());
        }
        Ok(())
    }).with_json_reviver(|key,value| {
        if key=="drop" { return Ok(None); }
        if key=="rejectReviver" { return Err("private reviver detail".into()); }
        Ok(Some(if key=="value" && value=="revive" {json!("revived")} else {value}))
    })).unwrap();
    for version in ["v1","v2"] {
        hooks.insert(&contracts()[6],version,forklaunch_http::VersionBodyHooks::default()
            .with_preprocess(|body| {
                let value=body["value"].as_str().ok_or("expected text")?;
                Ok(json!({"value":value.trim()}))
            })
            .with_transform(move |body| {
                let value=body["value"].as_str().ok_or("expected text")?;
                if value=="deny" || (version=="v2" && value=="blocked") { return Err("rejected".into()); }
                Ok(json!({"value":if version=="v1" {value.to_uppercase()} else {format!("{value}!")}}))
            })).unwrap();
    }
    router.with_version_body_hooks(hooks).post(contracts().remove(0),|Extension(version):Extension<SelectedVersion>,forklaunch_axum::VersionedBody(body):forklaunch_axum::VersionedBody<Value>|async move {
        (if version.version=="v1" {StatusCode::CREATED}else{StatusCode::ACCEPTED},Json(json!({"value":body["value"]})))
    }).post(contracts().remove(1),|Extension(version):Extension<SelectedVersion>,forklaunch_axum::VersionedBody(body):forklaunch_axum::VersionedBody<String>|async move {
        (if version.version=="v1" {StatusCode::CREATED}else{StatusCode::ACCEPTED},Json(json!({"version":version.version,"value":body})))
    }).post(contracts().remove(2),|Extension(version):Extension<SelectedVersion>,forklaunch_axum::VersionedBody(body):forklaunch_axum::VersionedBody<Vec<u8>>|async move {
        (StatusCode::ACCEPTED,Json(json!({"version":version.version,"bytes":body.to_vec()})))
    }).post(contracts().remove(3),|Extension(version):Extension<SelectedVersion>,body:String|async move {
        (match version.version.as_str() {"v1"=>StatusCode::CREATED,"v2"=>StatusCode::ACCEPTED,_=>StatusCode::OK},Json(json!({"version":version.version,"raw":body})))
    }).post(contracts().remove(4),|Extension(version):Extension<SelectedVersion>,body:axum::body::Bytes|async move {
        (if version.version=="v1" {StatusCode::CREATED}else{StatusCode::ACCEPTED},Json(json!({"version":version.version,"bytes":body.to_vec()})))
    }).post(contracts().remove(5),|Extension(version):Extension<SelectedVersion>,forklaunch_axum::VersionedBody(value):forklaunch_axum::VersionedBody<Value>|async move {
        (if version.version=="v1" {StatusCode::CREATED}else{StatusCode::ACCEPTED},Json(json!({"value":value})))
    }).post(contracts().remove(6),|Extension(version):Extension<SelectedVersion>,forklaunch_axum::VersionedBody(value):forklaunch_axum::VersionedBody<Value>|async move {
        (if version.version=="v1" {StatusCode::CREATED}else{StatusCode::ACCEPTED},Json(value))
    })
}
