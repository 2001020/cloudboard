//! PowerPoint build animations (`p:timing`), simplified to per-click steps
//! of fade-in (optionally sliding in), fade-out and a pulse for emphasis.

use crate::model::{Effect, Step};
use crate::opc::NodeExt;
use roxmltree::Node;

fn direction(preset: i64, sub: i64) -> (f32, f32) {
    // Fly in (2), Wipe (22), Peek in (12), Float in (42/47): slide in from a side.
    if !matches!(preset, 2 | 12 | 22 | 42 | 47) {
        return (0.0, 0.0);
    }
    match sub {
        1 => (0.0, -1.0), // from top
        2 => (1.0, 0.0),  // from right
        8 => (-1.0, 0.0), // from left
        _ => (0.0, 1.0),  // from bottom (4) and default
    }
}

/// Effects (shape id, effect) found under `n`, in document order.
fn effects(n: Node) -> Vec<(u32, Effect, String)> {
    let mut out = Vec::new();
    for ctn in n.descendants().filter(|d| d.has_tag_name_local("cTn") && d.attr("presetClass").is_some()) {
        let class = ctn.attr("presetClass").unwrap_or("");
        let preset = ctn.attr_i64("presetID").unwrap_or(0);
        let sub = ctn.attr_i64("presetSubtype").unwrap_or(0);
        let node_type = ctn.attr("nodeType").unwrap_or("").to_string();
        let effect = match class {
            "entr" => Effect::In { from: direction(preset, sub) },
            "exit" => Effect::Out,
            "emph" => Effect::Pulse,
            _ => continue,
        };
        let Some(spid) = ctn.descendants().find(|d| d.has_tag_name_local("spTgt")).and_then(|t| t.attr_i64("spid")) else { continue };
        if !out.iter().any(|(id, e, _): &(u32, Effect, String)| *id == spid as u32 && *e == effect) {
            out.push((spid as u32, effect, node_type));
        }
    }
    out
}

/// Parse a slide's timing tree into (effects run on slide entry, click steps).
pub fn parse(timing: Node) -> (Vec<(u32, Effect)>, Vec<Step>) {
    let mut auto = Vec::new();
    let mut steps: Vec<Step> = Vec::new();
    let Some(main) = timing.descendants().find(|d| d.has_tag_name_local("cTn") && d.attr("nodeType") == Some("mainSeq")) else { return (auto, steps) };
    let Some(list) = main.child("childTnLst") else { return (auto, steps) };
    for (i, par) in list.children_named("par").enumerate() {
        let effs = effects(par);
        if effs.is_empty() {
            continue;
        }
        let ctn = par.child("cTn");
        let click = ctn.and_then(|c| c.path(&["stCondLst", "cond"])).and_then(|c| c.attr("delay")).map(|d| d == "indefinite").unwrap_or(true) || effs.first().map(|e| e.2 == "clickEffect").unwrap_or(false);
        let list: Vec<(u32, Effect)> = effs.into_iter().map(|(a, b, _)| (a, b)).collect();
        if !click && i == 0 && steps.is_empty() {
            auto.extend(list);
        } else if !click && !steps.is_empty() {
            steps.last_mut().unwrap().effects.extend(list);
        } else {
            steps.push(Step { effects: list });
        }
    }
    (auto, steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<p:timing xmlns:p="p"><p:tnLst><p:par><p:cTn id="1" nodeType="tmRoot"><p:childTnLst>
      <p:seq><p:cTn id="2" nodeType="mainSeq"><p:childTnLst>
        <p:par><p:cTn id="3" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst>
          <p:par><p:cTn id="4"><p:childTnLst>
            <p:par><p:cTn id="5" presetID="10" presetClass="entr" nodeType="clickEffect"><p:childTnLst>
              <p:set><p:cBhvr><p:tgtEl><p:spTgt spid="4"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par>
            <p:par><p:cTn id="6" presetID="2" presetClass="entr" presetSubtype="8" nodeType="withEffect"><p:childTnLst>
              <p:set><p:cBhvr><p:tgtEl><p:spTgt spid="5"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par>
          </p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>
        <p:par><p:cTn id="7" fill="hold"><p:stCondLst><p:cond delay="indefinite"/></p:stCondLst><p:childTnLst>
          <p:par><p:cTn id="8"><p:childTnLst>
            <p:par><p:cTn id="9" presetID="10" presetClass="exit" nodeType="clickEffect"><p:childTnLst>
              <p:set><p:cBhvr><p:tgtEl><p:spTgt spid="4"/></p:tgtEl></p:cBhvr></p:set></p:childTnLst></p:cTn></p:par>
          </p:childTnLst></p:cTn></p:par></p:childTnLst></p:cTn></p:par>
      </p:childTnLst></p:cTn></p:seq></p:childTnLst></p:cTn></p:par></p:tnLst></p:timing>"#;

    #[test]
    fn click_steps() {
        let d = roxmltree::Document::parse(XML).unwrap();
        let (auto, steps) = parse(d.root_element());
        assert!(auto.is_empty());
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].effects, vec![(4, Effect::In { from: (0.0, 0.0) }), (5, Effect::In { from: (-1.0, 0.0) })]);
        assert_eq!(steps[1].effects, vec![(4, Effect::Out)]);
        let mut page = crate::model::Page::new(10.0, 10.0);
        page.steps = steps;
        assert_eq!(page.hidden_after(0).len(), 2);
        assert_eq!(page.hidden_after(1).len(), 0);
        assert!(page.hidden_after(2).contains(&4));
    }
}
