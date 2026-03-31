use chrono::Utc;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tracing::field::Visit;
use tracing_core::{Event, Subscriber};
use tracing_subscriber::fmt::{format::Writer, FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::registry::LookupSpan;

pub struct SutsV4Formatter {
    pub service_name: String,
    pub service_version: String,
    pub service_env: String,
}

impl<S, N> FormatEvent<S, N> for SutsV4Formatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let mut visitor = SutsVisitor::default();
        event.record(&mut visitor);

        let severity = match *meta.level() {
            tracing::Level::TRACE | tracing::Level::DEBUG => "DEBUG",
            tracing::Level::INFO => "INFO",
            tracing::Level::WARN => "WARN",
            tracing::Level::ERROR => "ERROR",
        };

        let event_name = visitor.event.unwrap_or_else(|| "LOG_EVENT".to_string());
        let message = visitor.message.unwrap_or_default();
        let mut attributes = visitor.fields;

        let trace_id = attributes.remove("trace_id").unwrap_or(Value::Null);
        let span_id = attributes.remove("span_id").unwrap_or(Value::Null);
        let tenant_id = attributes.remove("tenant_id").unwrap_or(Value::Null);
        let host_name = std::env::var("NODE_HOSTNAME").unwrap_or_else(|_| "unknown".to_string());

        // [ARCH-COMPLIANCE] Strict SUTS v4.0 JSON Şeması
        let log_obj = json!({
            "schema_v": "1.0.0",
            "ts": Utc::now().to_rfc3339(),
            "severity": severity,
            "tenant_id": tenant_id,
            "resource": {
                "service.name": self.service_name,
                "service.version": self.service_version,
                "service.env": self.service_env,
                "host.name": host_name
            },
            "trace_id": trace_id,
            "span_id": span_id,
            "event": event_name,
            "message": message,
            "attributes": attributes
        });

        // DÜZELTME: to_string() kaldırıldı, writeln! doğrudan JSON değerini işler
        writeln!(writer, "{log_obj}")
    }
}

#[derive(Default)]
struct SutsVisitor {
    message: Option<String>,
    event: Option<String>,
    fields: BTreeMap<String, Value>,
}

impl Visit for SutsVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let key = field.name();
        let val_str = format!("{:?}", value);
        let clean_val = val_str.trim_matches('"').to_string();
        if key == "message" {
            self.message = Some(clean_val);
        } else if key == "event" {
            self.event = Some(clean_val);
        } else {
            self.fields
                .insert(key.to_string(), Value::String(clean_val));
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        let key = field.name();
        if key == "message" {
            self.message = Some(value.to_string());
        } else if key == "event" {
            self.event = Some(value.to_string());
        } else {
            self.fields
                .insert(key.to_string(), Value::String(value.to_string()));
        }
    }
    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.insert(field.name().to_string(), json!(value));
    }
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.insert(field.name().to_string(), json!(value));
    }
    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields.insert(field.name().to_string(), json!(value));
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields.insert(field.name().to_string(), json!(value));
    }
}
