//! A local, opt-in telemetry effect script with bounded evaluation and fixed outputs.

use rhai::{AST, Array, Dynamic, Engine, Map, Scope};

use crate::telemetry::Sample;

pub const MAX_SOURCE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptCue {
    Pulse,
    Grit,
    Shift,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScriptEffect {
    pub light: Option<[u8; 3]>,
    pub cue: Option<ScriptCue>,
}

pub struct ScriptProgram {
    engine: Engine,
    ast: AST,
}

impl ScriptProgram {
    pub fn compile(source: &str) -> Result<Self, String> {
        if source.trim().is_empty() || source.len() > MAX_SOURCE_BYTES {
            return Err("Script must contain 1 to 4096 UTF-8 bytes.".into());
        }
        let mut engine = Engine::new();
        engine.set_max_operations(2_000);
        engine.set_max_expr_depths(24, 12);
        engine.set_max_call_levels(8);
        engine.set_max_variables(32);
        engine.set_max_functions(8);
        engine.set_max_array_size(16);
        engine.set_max_map_size(16);
        engine.set_max_string_size(256);
        engine.disable_symbol("eval");
        engine.disable_symbol("import");
        engine.on_print(|_| {});
        engine.on_debug(|_, _, _| {});
        let ast = engine
            .compile(source)
            .map_err(|e| format!("Script syntax: {e}"))?;
        Ok(Self { engine, ast })
    }

    pub fn evaluate(&self, sample: Sample) -> Result<ScriptEffect, String> {
        let mut scope = Scope::new();
        scope.push("rpm", f64::from(sample.rpm));
        scope.push("max_rpm", f64::from(sample.max_rpm));
        scope.push(
            "speed_kmh",
            sample.speed_mps.map_or(0.0, |v| f64::from(v) * 3.6),
        );
        scope.push("gear", sample.gear.map_or(-1, i64::from));
        scope.push("brake", sample.brake.map_or(0, i64::from));
        scope.push("slip", f64::from(sample.slip));
        scope.push("front_slip", f64::from(sample.front_slip));
        scope.push("kerb_left", sample.kerb[0] || sample.kerb[2]);
        scope.push("kerb_right", sample.kerb[1] || sample.kerb[3]);
        let output: Map = self
            .engine
            .eval_ast_with_scope(&mut scope, &self.ast)
            .map_err(|e| format!("Script stopped: {e}"))?;
        parse_effect(&output)
    }
}

fn parse_effect(output: &Map) -> Result<ScriptEffect, String> {
    if output.keys().any(|key| key != "light" && key != "cue") {
        return Err("Script can return only light and cue.".into());
    }
    let light = output.get("light").map(parse_color).transpose()?;
    let cue = output
        .get("cue")
        .map(|value| -> Result<ScriptCue, String> {
            let label = value
                .clone()
                .try_cast::<rhai::ImmutableString>()
                .ok_or_else(|| "Cue must be pulse, grit, or shift.".to_string())?;
            match label.as_str() {
                "pulse" => Ok(ScriptCue::Pulse),
                "grit" => Ok(ScriptCue::Grit),
                "shift" => Ok(ScriptCue::Shift),
                _ => Err("Cue must be pulse, grit, or shift.".into()),
            }
        })
        .transpose()?;
    Ok(ScriptEffect { light, cue })
}

fn parse_color(value: &Dynamic) -> Result<[u8; 3], String> {
    let array = value
        .clone()
        .try_cast::<Array>()
        .ok_or("Light must be an RGB array.")?;
    if array.len() != 3 {
        return Err("Light needs exactly three RGB values.".into());
    }
    let mut color = [0u8; 3];
    for (channel, value) in color.iter_mut().zip(array) {
        *channel = value
            .as_int()
            .ok()
            .and_then(|v| u8::try_from(v).ok())
            .ok_or("Light RGB values must be integers from 0 to 255.")?;
    }
    Ok(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        Sample {
            rpm: 7200.0,
            max_rpm: 8000.0,
            speed_mps: Some(30.0),
            gear: Some(4),
            brake: Some(0),
            slip: 0.0,
            front_slip: 0.0,
            kerb: [false; 4],
            source: "Test",
        }
    }

    #[test]
    fn script_uses_bounded_signals_and_outputs() {
        let script = ScriptProgram::compile(
            "if rpm / max_rpm > 0.8 { #{ light: [220, 40, 20], cue: \"pulse\" } } else { #{} }",
        )
        .unwrap();
        assert_eq!(
            script.evaluate(sample()).unwrap(),
            ScriptEffect {
                light: Some([220, 40, 20]),
                cue: Some(ScriptCue::Pulse)
            }
        );
    }

    #[test]
    fn script_stops_loops_and_rejects_unsafe_outputs() {
        let loop_script = ScriptProgram::compile("loop {} #{ light: [1, 2, 3] }").unwrap();
        assert!(loop_script.evaluate(sample()).is_err());
        let invalid = ScriptProgram::compile("#{ light: [300, 0, 0] }").unwrap();
        assert!(invalid.evaluate(sample()).is_err());
        let unknown = ScriptProgram::compile("#{ trigger: 100 }").unwrap();
        assert!(unknown.evaluate(sample()).is_err());
    }
}
