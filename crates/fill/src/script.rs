//! Turning a plan into the code that types it into the page.
//!
//! Every value is emitted as a JSON string literal, so a profile field can
//! never become code. A name containing a quote is a name, not an injection.
//!
//! The generated script sets values and dispatches the events a form needs to
//! notice them. It does not activate anything. `emits_nothing_that_submits`
//! and `no_form_is_ever_activated` in this module's tests read the emitted text
//! and assert that, so the promise is checked against what actually runs.
//!
//! It also ends by handing back an account of what it managed to do. Nothing
//! on the far side of the fill can see the page, so a script that threw on its
//! first statement and a script that filled every box looked the same from
//! here for as long as there was nothing to hand back. The tests below that
//! call `run` execute the emitted text rather than reading it, because reading
//! it is what let that happen.

use crate::{Action, FillPlan};
use std::collections::BTreeMap;

/// A file the person chose, read for handing to the page.
#[derive(Debug, Clone)]
pub struct Attachment {
    pub name: String,
    pub mime: String,
    /// The file's bytes, base64-encoded.
    pub base64: String,
}

/// A JavaScript string literal that cannot escape its own quotes, and cannot
/// close a surrounding script element either.
fn literal(value: &str) -> String {
    serde_json::to_string(value)
        .unwrap_or_else(|_| "\"\"".into())
        .replace("</", "<\\/")
}

/// The code Perch runs in the application form's page.
///
/// `files` is keyed by selector; a selector with no attachment is skipped
/// rather than cleared.
pub fn to_script(plan: &FillPlan, files: &BTreeMap<String, Attachment>) -> String {
    let mut values = String::new();
    // Counted here so a script that throws before it has read its own plan can
    // still say how many values it was carrying.
    let mut planned = 0usize;
    for entry in &plan.entries {
        match &entry.action {
            Action::SetText {
                selector,
                labels,
                value,
            } => {
                let words: Vec<String> = labels.iter().map(|l| literal(l)).collect();
                values.push_str(&format!(
                    "    {{ s: {}, n: {}, l: [{}], v: {} }},\n",
                    literal(selector),
                    literal(&entry.label),
                    words.join(", "),
                    literal(value)
                ));
                planned += 1;
            }
            Action::SetChoice { selector, value } => {
                values.push_str(&format!(
                    "    {{ s: {}, n: {}, l: [], v: {} }},\n",
                    literal(selector),
                    literal(&entry.label),
                    literal(value)
                ));
                planned += 1;
            }
            Action::AttachFile { .. } => {}
        }
    }

    let mut attachments = String::new();
    for entry in &plan.entries {
        if let Action::AttachFile { selector, .. } = &entry.action {
            let Some(file) = files.get(selector) else {
                continue;
            };
            attachments.push_str(&format!(
                "    {{ s: {}, name: {}, mime: {}, b64: {} }},\n",
                literal(selector),
                literal(&file.name),
                literal(&file.mime),
                literal(&file.base64)
            ));
        }
    }

    let file_planned = if attachments.is_empty() {
        "false"
    } else {
        "true"
    };

    let mut left_alone = String::new();
    for flagged in &plan.flagged {
        left_alone.push_str(&format!(
            "  // left empty on purpose: {}\n",
            flagged.label.replace('\n', " ")
        ));
    }
    for free in &plan.left_to_you {
        left_alone.push_str(&format!(
            "  // yours to write: {}\n",
            free.label.replace('\n', " ")
        ));
    }
    for refused in &plan.never {
        left_alone.push_str(&format!(
            "  // never answered: {}\n",
            refused.label.replace('\n', " ")
        ));
    }

    let markers: Vec<String> = crate::flavor::DEMOGRAPHIC_MARKERS
        .iter()
        .map(|m| literal(m))
        .collect();
    let markers = markers.join(", ");

    format!(
        "// Written by Perch from a plan you read first. It types values and stops.\n\
         // There is no submit in here, and no click: Perch does not send applications.\n\
         // It ends by handing back an account of what happened, because the side\n\
         // that asked for the fill cannot see the page, and a page that typed\n\
         // nothing looks exactly like one that typed everything from there.\n\
         (function () {{\n\
         \x20try {{\n\
         \x20 var plan = [\n{values}  ];\n\
         \x20 var files = [\n{attachments}  ];\n\
{left_alone}\
         \x20 var markers = [{markers}];\n\
         \x20 // Perch's own mark on the account it leaves behind. The page can\n\
         \x20 // write to window as freely as this script can, and the side that\n\
         \x20 // reads the account back has nothing else to tell an answer Perch\n\
         \x20 // wrote from one the page wrote under the same name.\n\
         \x20 var stamp = 'perch-' + Math.random().toString(36).slice(2)\n\
         \x20   + Math.random().toString(36).slice(2);\n\
         \x20 var trouble = null;\n\
         \x20 var handed = [];\n\
         \x20 function norm(text) {{\n\
         \x20   return String(text).replace(/\\s+/g, ' ').trim()\n\
         \x20     .replace(/\\*$/, '').trim().toLowerCase();\n\
         \x20 }}\n\
         \x20 // A question Perch will not answer, read off the page rather\n\
         \x20 // than off a list of boxes Perch already knew about.\n\
         \x20 function refused(text) {{\n\
         \x20   var t = norm(text);\n\
         \x20   for (var i = 0; i < markers.length; i++) {{\n\
         \x20     if (t.indexOf(markers[i]) !== -1) {{ return true; }}\n\
         \x20   }}\n\
         \x20   return false;\n\
         \x20 }}\n\
         \x20 // Only boxes you type into. Every demographic question on the\n\
         \x20 // boards Perch fills is a radio, a checkbox or a select, so this\n\
         \x20 // alone puts them out of reach whatever they are called.\n\
         \x20 function typeable(el) {{\n\
         \x20   if (el.tagName === 'TEXTAREA') {{ return true; }}\n\
         \x20   if (el.tagName !== 'INPUT') {{ return false; }}\n\
         \x20   var t = (el.type || 'text').toLowerCase();\n\
         \x20   return t === 'text' || t === 'email' || t === 'tel'\n\
         \x20     || t === 'url' || t === 'search';\n\
         \x20 }}\n\
         \x20 // The box a label points at. A `for` naming nothing falls back to\n\
         \x20 // the box the label is printed with: Ashby writes `for` on its\n\
         \x20 // Location label and renders a combobox with no id for it to\n\
         \x20 // name. One box or none, so a container holding several is never\n\
         \x20 // guessed between.\n\
         \x20 function boxFor(label) {{\n\
         \x20   if (label.htmlFor) {{\n\
         \x20     var target = document.getElementById(label.htmlFor);\n\
         \x20     if (target) {{ return target; }}\n\
         \x20   }}\n\
         \x20   var inside = label.querySelectorAll('input, textarea');\n\
         \x20   if (inside.length) {{ return inside.length === 1 ? inside[0] : null; }}\n\
         \x20   var near = label.parentElement\n\
         \x20     ? label.parentElement.querySelectorAll('input, textarea')\n\
         \x20     : [];\n\
         \x20   return near.length === 1 ? near[0] : null;\n\
         \x20 }}\n\
         \x20 // The heading a box sits under. A question inside a\n\
         \x20 // self-identification block can be labelled Name, and the label\n\
         \x20 // alone does not say which form it belongs to. The search stops\n\
         \x20 // at the form, and a few levels up in any case, so a box is\n\
         \x20 // never judged by the job's title.\n\
         \x20 function heading(el) {{\n\
         \x20   var node = el.parentElement, depth = 0;\n\
         \x20   while (node && node !== document.body && depth < 6) {{\n\
         \x20     var marks = node.querySelectorAll('h1, h2, h3, h4, h5, h6, legend');\n\
         \x20     for (var i = marks.length - 1; i >= 0; i--) {{\n\
         \x20       var order = marks[i].compareDocumentPosition(el);\n\
         \x20       if (order & Node.DOCUMENT_POSITION_FOLLOWING) {{\n\
         \x20         return marks[i].textContent;\n\
         \x20       }}\n\
         \x20     }}\n\
         \x20     if (node.tagName === 'FORM') {{ return ''; }}\n\
         \x20     node = node.parentElement;\n\
         \x20     depth++;\n\
         \x20   }}\n\
         \x20   return '';\n\
         \x20 }}\n\
         \x20 // Every word the page uses to name a box: each label pointing at\n\
         \x20 // it, and the heading above it. Reading only the label that\n\
         \x20 // matched reads Perch's own wording back, because matching is\n\
         \x20 // what made the two the same. So the box is what gets checked.\n\
         \x20 function refusedBox(el) {{\n\
         \x20   var tags = document.querySelectorAll('label');\n\
         \x20   for (var i = 0; i < tags.length; i++) {{\n\
         \x20     if (boxFor(tags[i]) !== el) {{ continue; }}\n\
         \x20     if (refused(tags[i].textContent)) {{ return true; }}\n\
         \x20   }}\n\
         \x20   return refused(heading(el));\n\
         \x20 }}\n\
         \x20 // The box a label names, when exactly one label says it and it\n\
         \x20 // names exactly one box worth typing into. Two boxes with the\n\
         \x20 // same words is not a match Perch can be sure of, so it types\n\
         \x20 // nothing and the person fills that one themselves.\n\
         \x20 function byLabel(labels) {{\n\
         \x20   for (var i = 0; i < labels.length; i++) {{\n\
         \x20     var tags = document.querySelectorAll('label');\n\
         \x20     var hit = null, count = 0;\n\
         \x20     for (var j = 0; j < tags.length; j++) {{\n\
         \x20       if (norm(tags[j].textContent) !== norm(labels[i])) {{ continue; }}\n\
         \x20       var el = boxFor(tags[j]);\n\
         \x20       if (!el || !typeable(el)) {{ continue; }}\n\
         \x20       if (refusedBox(el)) {{ return null; }}\n\
         \x20       count++;\n\
         \x20       hit = el;\n\
         \x20     }}\n\
         \x20     if (count === 1) {{ return hit; }}\n\
         \x20   }}\n\
         \x20   return null;\n\
         \x20 }}\n\
         \x20 // The selector first: a name the board chose holds better than\n\
         \x20 // words a company typed into its own form.\n\
         \x20 function box(entry) {{\n\
         \x20   if (entry.s) {{\n\
         \x20     var found = document.querySelector(entry.s);\n\
         \x20     if (found) {{ return found; }}\n\
         \x20   }}\n\
         \x20   return entry.l && entry.l.length ? byLabel(entry.l) : null;\n\
         \x20 }}\n\
         \x20 function put(el, value) {{\n\
         \x20   var proto = el instanceof HTMLTextAreaElement\n\
         \x20     ? HTMLTextAreaElement.prototype\n\
         \x20     : el instanceof HTMLSelectElement\n\
         \x20       ? HTMLSelectElement.prototype\n\
         \x20       : HTMLInputElement.prototype;\n\
         \x20   var setter = Object.getOwnPropertyDescriptor(proto, 'value');\n\
         \x20   if (setter && setter.set) {{ setter.set.call(el, value); }} else {{ el.value = value; }}\n\
         \x20   el.dispatchEvent(new Event('input', {{ bubbles: true }}));\n\
         \x20   el.dispatchEvent(new Event('change', {{ bubbles: true }}));\n\
         \x20 }}\n\
         \x20 function attach(el, f) {{\n\
         \x20   var raw = atob(f.b64);\n\
         \x20   var bytes = new Uint8Array(raw.length);\n\
         \x20   for (var i = 0; i < raw.length; i++) {{ bytes[i] = raw.charCodeAt(i); }}\n\
         \x20   var transfer = new DataTransfer();\n\
         \x20   transfer.items.add(new File([bytes], f.name, {{ type: f.mime }}));\n\
         \x20   el.files = transfer.files;\n\
         \x20   tell(el);\n\
         \x20 }}\n\
         \x20 // Putting the file on the box is not the same as the form knowing\n\
         \x20 // it is there. The form is built after the page loads, so the\n\
         \x20 // first telling can land before anything is listening, and a file\n\
         \x20 // nobody heard about sits on the box unread while the page still\n\
         \x20 // says Attach. So it is said again until the form answers.\n\
         \x20 // Whether the form has printed the file's name anywhere. It is\n\
         \x20 // the one sign all three boards give, and a board already listing\n\
         \x20 // a document of the same name gives it without having taken this\n\
         \x20 // one, so it is reported as the words it is and nothing more.\n\
         \x20 function took(f) {{\n\
         \x20   return document.body.textContent.indexOf(f.name) !== -1;\n\
         \x20 }}\n\
         \x20 function tell(el) {{\n\
         \x20   el.dispatchEvent(new Event('change', {{ bubbles: true }}));\n\
         \x20 }}\n\
         \x20 // Whether the form has named every planned file back.\n\
         \x20 function allNamed() {{\n\
         \x20   for (var m = 0; m < files.length; m++) {{\n\
         \x20     if (!took(files[m])) {{ return false; }}\n\
         \x20   }}\n\
         \x20   return files.length > 0;\n\
         \x20 }}\n\
         \x20 // Whether every planned file was handed to a box. Perch did that\n\
         \x20 // or it did not, which is the part of an upload it can answer for\n\
         \x20 // without taking the page's word for anything.\n\
         \x20 function allHanded() {{\n\
         \x20   for (var h = 0; h < files.length; h++) {{\n\
         \x20     if (!handed[h]) {{ return false; }}\n\
         \x20   }}\n\
         \x20   return files.length > 0;\n\
         \x20 }}\n\
         \x20 function plain(text) {{\n\
         \x20   return String(text).replace(/[^0-9a-z]+/gi, '').toLowerCase();\n\
         \x20 }}\n\
         \x20 // Whether a box is holding the value it was handed. A phone box\n\
         \x20 // that prints (415) 555-0148 back holds the number it was given,\n\
         \x20 // and counting it as empty would name it to the person as a box\n\
         \x20 // they still have to fill in themselves.\n\
         \x20 function holds(el, want) {{\n\
         \x20   var has = el.value === null || el.value === undefined\n\
         \x20     ? '' : String(el.value);\n\
         \x20   if (has === want) {{ return true; }}\n\
         \x20   var a = plain(has), b = plain(want);\n\
         \x20   return a !== '' && b !== ''\n\
         \x20     && (a.indexOf(b) !== -1 || b.indexOf(a) !== -1);\n\
         \x20 }}\n\
         \x20 // The first thing that went wrong is the one that explains the\n\
         \x20 // rest, so a later pass does not write over it.\n\
         \x20 function note(err) {{\n\
         \x20   if (trouble === null) {{\n\
         \x20     trouble = String((err && err.message) || err);\n\
         \x20   }}\n\
         \x20 }}\n\
         \x20 // What the form holds now, read off the boxes rather than off a\n\
         \x20 // memory of what was typed into them. A value the page threw away\n\
         \x20 // after Perch wrote it is a value that is not there.\n\
         \x20 function tally() {{\n\
         \x20   var here = 0;\n\
         \x20   var lost = [];\n\
         \x20   var other = [];\n\
         \x20   for (var k = 0; k < plan.length; k++) {{\n\
         \x20     var found = box(plan[k]);\n\
         \x20     if (!found) {{ lost.push(plan[k].n); continue; }}\n\
         \x20     if (holds(found, plan[k].v)) {{ here++; }}\n\
         \x20     else {{ other.push(plan[k].n); }}\n\
         \x20   }}\n\
         \x20   return {{ stamp: stamp, filled: here, planned: plan.length,\n\
         \x20     missing: lost, changed: other, file: files.length > 0,\n\
         \x20     fileGiven: allHanded(), fileNamed: allNamed(),\n\
         \x20     settled: !running, error: trouble }};\n\
         \x20 }}\n\
         \x20 // The account, taken when it is asked for rather than when the\n\
         \x20 // loop last ran. A form that emptied itself after the loop\n\
         \x20 // stopped is a form with nothing in it, and a count from six\n\
         \x20 // seconds earlier would say otherwise. A page broken past\n\
         \x20 // counting leaves the count out rather than inventing one.\n\
         \x20 function reading() {{\n\
         \x20   try {{\n\
         \x20     return tally();\n\
         \x20   }} catch (err) {{\n\
         \x20     return {{ stamp: stamp, filled: 0, planned: {planned},\n\
         \x20       missing: [], changed: [], file: {file_planned},\n\
         \x20       fileGiven: false, fileNamed: false, settled: !running,\n\
         \x20       error: trouble || String((err && err.message) || err) }};\n\
         \x20   }}\n\
         \x20 }}\n\
         \x20 // Most application forms build themselves after the page loads.\n\
         \x20 // Filling one before it exists does nothing, and a form's first\n\
         \x20 // render can reset what was already filled. So this keeps putting\n\
         \x20 // the same values back until they stay put, for a few seconds,\n\
         \x20 // and then stops. What it puts back is a box the form emptied,\n\
         \x20 // never a box holding different words, so a correction made\n\
         \x20 // while this is still running is not undone.\n\
         \x20 var tries = 0;\n\
         \x20 var began = Date.now();\n\
         \x20 var running = true;\n\
         \x20 var seen = [];\n\
         \x20 var told = [];\n\
         \x20 var wrote = [];\n\
         \x20 var first = null;\n\
         \x20 // Where the account is read from once the loop has stopped. It\n\
         \x20 // is defined rather than assigned, so that nothing in the page\n\
         \x20 // can put its own answer under the name afterwards.\n\
         \x20 try {{\n\
         \x20   Object.defineProperty(window, '__perchFill', {{\n\
         \x20     get: reading, configurable: false\n\
         \x20   }});\n\
         \x20 }} catch (err) {{\n\
         \x20   // The page put something under this name before the fill ran,\n\
         \x20   // and nothing here can move it. Its answer carries no stamp,\n\
         \x20   // so it is not read as an account of this fill.\n\
         \x20 }}\n\
         \x20 (function settle() {{\n\
         \x20   var waiting = 0;\n\
         \x20   try {{\n\
         \x20     for (var i = 0; i < plan.length; i++) {{\n\
         \x20       var el = box(plan[i]);\n\
         \x20       if (!el) {{ waiting++; continue; }}\n\
         \x20       if (el.value === plan[i].v) {{ wrote[i] = true; continue; }}\n\
         \x20       // Written once, and again only if the form emptied it. A\n\
         \x20       // box holding something else after Perch filled it is a\n\
         \x20       // change the person made, and theirs is the copy that\n\
         \x20       // stands.\n\
         \x20       if (wrote[i] && el.value !== '') {{ continue; }}\n\
         \x20       put(el, plan[i].v);\n\
         \x20       wrote[i] = true;\n\
         \x20       waiting++;\n\
         \x20     }}\n\
         \x20     for (var j = 0; j < files.length; j++) {{\n\
         \x20       var slot = document.querySelector(files[j].s);\n\
         \x20       // A box that has gone after being taken is settled. A box\n\
         \x20       // that has not appeared yet is a form still being built,\n\
         \x20       // which is not the same thing.\n\
         \x20       if (!slot) {{ if (!seen[j]) {{ waiting++; }} continue; }}\n\
         \x20       seen[j] = true;\n\
         \x20       if (!('files' in slot)) {{ waiting++; continue; }}\n\
         \x20       if (!slot.files || slot.files.length === 0) {{\n\
         \x20         // What the page prints decides nothing here. A board\n\
         \x20         // already listing a document of this name would keep the\n\
         \x20         // person's file from ever being handed over.\n\
         \x20         attach(slot, files[j]);\n\
         \x20         handed[j] = true;\n\
         \x20         told[j] = tries;\n\
         \x20         waiting++;\n\
         \x20       }} else if (!took(files[j])) {{\n\
         \x20         waiting++;\n\
         \x20         if (tries - told[j] >= 4) {{\n\
         \x20           // The file is on the box and the form has not named\n\
         \x20           // it. Say it again, about once a second rather than\n\
         \x20           // every pass, so a form that did hear is not asked to\n\
         \x20           // upload twice.\n\
         \x20           tell(slot);\n\
         \x20           told[j] = tries;\n\
         \x20         }}\n\
         \x20       }}\n\
         \x20     }}\n\
         \x20   }} catch (err) {{\n\
         \x20     // Every pass after the first runs from a timer of its own,\n\
         \x20     // where a throw reaches no catch at all: the loop stopped\n\
         \x20     // and the account still read as one that finished with\n\
         \x20     // nothing wrong. The passes left still run, because one box\n\
         \x20     // the page took apart is not the rest of the form.\n\
         \x20     note(err);\n\
         \x20     waiting++;\n\
         \x20   }}\n\
         \x20   tries++;\n\
         \x20   // Seconds of the clock rather than a count of passes: a window\n\
         \x20   // behind another one has its timers slowed to one a second,\n\
         \x20   // and counting passes would leave the loop still writing long\n\
         \x20   // after the account had been read and shown to the person.\n\
         \x20   if (waiting > 0 && tries < 24 && Date.now() - began < 6000) {{\n\
         \x20     setTimeout(settle, 250);\n\
         \x20   }} else {{ running = false; }}\n\
         \x20   if (first === null) {{ first = reading(); }}\n\
         \x20 }})();\n\
         \x20 return JSON.stringify(first);\n\
         \x20}} catch (err) {{\n\
         \x20 // The evaluator drops exceptions, so one that stopped the fill\n\
         \x20 // leaves an empty form and no word of why. It goes out as text,\n\
         \x20 // with the boxes read rather than assumed: a throw can come\n\
         \x20 // after values have already been typed into them.\n\
         \x20 note(err);\n\
         \x20 return JSON.stringify(reading());\n\
         \x20}}\n\
         }})();\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan;
    use perch_core::{profile::Links, Ats, Profile};

    fn profile() -> Profile {
        Profile {
            name: "Dana Ferreira".into(),
            email: "dana@dferreira.dev".into(),
            phone: "+1 415 555 0148".into(),
            location: "Oakland, California".into(),
            links: Links {
                github: "github.com/dferreira".into(),
                website: "dferreira.dev".into(),
                linkedin: "linkedin.com/in/dferreira".into(),
            },
            ..Profile::default()
        }
    }

    fn script_for(ats: Ats) -> String {
        let plan = plan::build(ats, "https://x.invalid", &profile(), Some("/docs/cv.pdf")).unwrap();
        let mut files = BTreeMap::new();
        for entry in &plan.entries {
            if let Action::AttachFile { selector, .. } = &entry.action {
                files.insert(
                    selector.clone(),
                    Attachment {
                        name: "cv.pdf".into(),
                        mime: "application/pdf".into(),
                        base64: "JVBERi0xLjQK".into(),
                    },
                );
            }
        }
        to_script(&plan, &files)
    }

    #[test]
    fn emits_nothing_that_submits() {
        // The promise, checked against the text that actually runs.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let script = script_for(ats);
            for forbidden in [
                ".submit(",
                "requestSubmit",
                "HTMLFormElement.prototype.submit",
                "form.submit",
                "type=\"submit\"",
                "[type=submit]",
            ] {
                assert!(
                    !script.contains(forbidden),
                    "{ats:?} script contains {forbidden:?}"
                );
            }
        }
    }

    #[test]
    fn no_form_is_ever_activated() {
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let script = script_for(ats);
            for forbidden in [
                ".click(",
                "click()",
                "MouseEvent",
                "PointerEvent",
                "dispatchEvent(new Event('submit'",
                "fetch(",
                "XMLHttpRequest",
                "navigator.sendBeacon",
            ] {
                assert!(
                    !script.contains(forbidden),
                    "{ats:?} script contains {forbidden:?}"
                );
            }
        }
    }

    #[test]
    fn a_value_containing_quotes_stays_a_value() {
        let mut profile = profile();
        // Everything someone might have in a profile, or import into one.
        profile.name = r#"Dana "D" O'Ferreira\"#.into();
        profile.location = "Oakland</script><script>alert(1)</script>".into();
        profile.email = "a\"); alert(1); //@example.com".into();

        let plan = plan::build(Ats::Lever, "https://x.invalid", &profile, None).unwrap();
        let script = to_script(&plan, &BTreeMap::new());

        // The dangerous text is still present, because it is the person's
        // data. It appears only as an argument to `put`, never as a statement.
        for line in script.lines() {
            if line.contains("alert(1)") {
                assert!(
                    line.trim_start().starts_with("{ s:"),
                    "a value escaped its argument:\n{line}"
                );
            }
        }
        assert!(
            !script.contains("</script>"),
            "a value could close a script element"
        );
        assert!(
            script.contains(r#"<\/script>"#),
            "the closing tag was not escaped"
        );
        // And the values are still there, as strings.
        assert!(script.contains(r#"Dana \"D\" O'Ferreira\\"#));
    }

    #[test]
    fn a_newline_in_a_value_cannot_start_a_new_statement() {
        let mut profile = profile();
        profile.location = "Oakland\n  window.location = 'https://example.invalid';".into();
        let plan = plan::build(Ats::Lever, "https://x.invalid", &profile, None).unwrap();
        let script = to_script(&plan, &BTreeMap::new());
        // The newline is escaped, so the whole thing is one string literal.
        assert!(script.contains("\\n"));
        for line in script.lines() {
            assert!(
                !line.trim_start().starts_with("window.location"),
                "a value became a statement:\n{script}"
            );
        }
    }

    #[test]
    fn every_planned_value_appears_exactly_once() {
        let plan = plan::build(Ats::Ashby, "https://x.invalid", &profile(), None).unwrap();
        let script = to_script(&plan, &BTreeMap::new());
        // Keyed on the value, not the selector: a box found by the words
        // beside it has no selector to key on, and three of Ashby's do.
        for entry in &plan.entries {
            let listed = format!("v: {}", literal(entry.action.shown_value()));
            assert_eq!(
                script.matches(&listed).count(),
                1,
                "{:?} appears in the plan {} times",
                entry.label,
                script.matches(&listed).count()
            );
        }
    }

    #[test]
    fn the_deliberate_blanks_are_named_in_the_script_itself() {
        // Anyone reading the generated code should see that the empty boxes
        // were decided on, not missed.
        let plan = plan::build(Ats::Ashby, "https://x.invalid", &profile(), None).unwrap();
        let script = to_script(&plan, &BTreeMap::new());
        assert!(script.contains("// left empty on purpose: Preferred start date"));
        assert!(script.contains("// yours to write: Why do you want to work here"));
        assert!(script.contains("// never answered: Gender"));
    }

    #[test]
    fn a_box_found_by_its_label_is_only_ever_one_you_type_into() {
        // Matching on the words beside a box is how Ashby's own questions are
        // reached, and it is also how a demographic question would be reached
        // by accident. Every one of them on the boards Perch fills is a radio,
        // a checkbox or a select, so the fill refuses anything else outright,
        // before any word is looked at.
        let plan = plan::build(Ats::Ashby, "https://x.invalid", &profile(), None).unwrap();
        let script = to_script(&plan, &BTreeMap::new());
        assert!(script.contains("function typeable("));
        for kind in ["radio", "checkbox", "select", "file"] {
            assert!(
                !script.contains(&format!("t === '{kind}'")),
                "the fill would type into a {kind}"
            );
        }
        for kind in ["text", "email", "tel", "url", "search"] {
            assert!(script.contains(&format!("t === '{kind}'")));
        }
    }

    #[test]
    fn the_words_on_the_page_decide_what_is_refused_not_perchs_own_table() {
        // The label check moved into the page along with the matching. A
        // question Perch has never seen, on a board that names its boxes per
        // posting, is still not answered on anyone's behalf.
        let plan = plan::build(Ats::Ashby, "https://x.invalid", &profile(), None).unwrap();
        let script = to_script(&plan, &BTreeMap::new());
        assert!(script.contains("function refused("));
        assert!(
            script.contains("if (refusedBox(el)) { return null; }"),
            "a refused box does not stop the match"
        );
        // The same list Rust checks, carried in rather than written twice.
        for marker in crate::flavor::DEMOGRAPHIC_MARKERS {
            assert!(
                script.contains(&format!("\"{marker}\"")),
                "{marker:?} is missing from the list the page checks"
            );
        }
    }

    #[test]
    fn two_boxes_with_the_same_words_are_left_alone() {
        // A label Perch cannot resolve to one box is a label it does not act
        // on. Guessing between them is how the wrong question gets answered.
        let plan = plan::build(Ats::Ashby, "https://x.invalid", &profile(), None).unwrap();
        let script = to_script(&plan, &BTreeMap::new());
        assert!(script.contains("if (count === 1) { return hit; }"));
    }

    #[test]
    fn no_demographic_answer_reaches_the_page() {
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let script = script_for(ats);
            for line in script
                .lines()
                .filter(|l| l.trim_start().starts_with("put("))
            {
                for marker in ["gender", "ethnic", "race", "veteran", "disability"] {
                    assert!(
                        !line.to_lowercase().contains(marker),
                        "{ats:?} would type into {line}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_file_the_form_has_not_taken_is_offered_again() {
        // The bug this guards against, found by driving a real Greenhouse form:
        // the box is in the page's own HTML and the form is wired up afterwards,
        // so the first telling landed before anything was listening. The file
        // sat on the box, the loop saw a box that had a file and asked nothing
        // further, and the form said Attach for as long as it was open.
        let plan = plan::build(
            Ats::Greenhouse,
            "https://boards.greenhouse.io/x/jobs/1",
            &profile(),
            Some("/docs/cv.pdf"),
        )
        .unwrap();
        let mut files = BTreeMap::new();
        for entry in &plan.entries {
            if let Action::AttachFile { selector, .. } = &entry.action {
                files.insert(
                    selector.clone(),
                    Attachment {
                        name: "cv.pdf".into(),
                        mime: "application/pdf".into(),
                        base64: "QQ==".into(),
                    },
                );
            }
        }
        let script = to_script(&plan, &files);

        // A box already holding the file is still told about it again.
        assert!(
            script.contains("tell(slot)"),
            "a file the form never heard about is never mentioned again"
        );
        // Spaced out, so a form that did hear is not made to upload repeatedly.
        assert!(script.contains("told[j] >= 4"));
        // And a box that has gone is the form saying it took the file, which is
        // not the same as a box that has not been built yet.
        assert!(script.contains("if (!seen[j])"));
    }

    #[test]
    fn a_file_the_form_has_named_back_is_not_offered_again() {
        // Greenhouse takes its box away once the upload lands, Lever and Ashby
        // leave it sitting there with the file still on it. Telling those two
        // again on every pass made them upload the same résumé over and over,
        // so what all three agree on is used instead: the form prints the
        // file's name.
        let plan = plan::build(
            Ats::Lever,
            "https://jobs.lever.co/x/1/apply",
            &profile(),
            Some("/docs/cv.pdf"),
        )
        .unwrap();
        let mut files = BTreeMap::new();
        for entry in &plan.entries {
            if let Action::AttachFile { selector, .. } = &entry.action {
                files.insert(
                    selector.clone(),
                    Attachment {
                        name: "cv.pdf".into(),
                        mime: "application/pdf".into(),
                        base64: "QQ==".into(),
                    },
                );
            }
        }
        let script = to_script(&plan, &files);

        // Named back on the first pass, so the box holds the file and the
        // form is never told about it a second time.
        let run = run_on(
            &script,
            r#"{ "boxes": [{ "sel": "input[name='resume']", "type": "file" }],
                 "namesFiles": true }"#,
        );
        assert_eq!(names_on(&run, "input[name='resume']"), vec!["cv.pdf"]);
        assert_eq!(
            told_again(&run, "input[name='resume']"),
            0,
            "a form that named the file back was told about it again"
        );
        assert_eq!(report(run["read"].as_str().unwrap())["fileNamed"], true);
    }

    /// The names the script declares as functions, and the names it declares
    /// with `var`. A name in both lists is a hoisted variable standing in
    /// front of a function for the whole of the function it sits in.
    fn declared(script: &str) -> (Vec<String>, Vec<String>) {
        let mut functions = Vec::new();
        let mut vars = Vec::new();
        for line in script.lines() {
            let line = line.trim();
            if line.starts_with("//") {
                continue;
            }
            if let Some(rest) = line.split("function ").nth(1) {
                let name = rest.split('(').next().unwrap_or_default().trim();
                if !name.is_empty() {
                    functions.push(name.to_string());
                }
            }
            if let Some(rest) = line.split("var ").nth(1) {
                for declarator in rest.split(',') {
                    let name: String = declarator
                        .trim()
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        vars.push(name);
                    }
                }
            }
        }
        (functions, vars)
    }

    #[test]
    fn no_variable_in_the_script_stands_in_front_of_one_of_its_functions() {
        // `var` is scoped to the whole function it appears in and is hoisted
        // to the top of it, so `var box` in the file loop stood in front of
        // `function box(entry)` from the first line of `settle`, and the
        // lookup above it called undefined. Nothing was typed on any board.
        // Every test here passed throughout, because they all read the
        // emitted text and none of them ran it, so this reads the shape of
        // the text instead of one line of it.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let script = script_for(ats);
            let (functions, vars) = declared(&script);
            assert!(functions.contains(&"box".to_string()));
            for name in &vars {
                assert!(
                    !functions.contains(name),
                    "{ats:?}: `var {name}` stands in front of `function {name}(`, \
                     so calling {name} calls undefined"
                );
            }
        }
    }

    #[test]
    fn a_label_perch_matched_is_never_what_decides_the_refusal() {
        // The check sat after the page's words had been found equal to
        // Perch's own, so the only text it could read was text Perch wrote,
        // and none of that text is a marker: it could not return true. What
        // it reads now is the box those words resolved to, which the page
        // names in its own words.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let plan = plan::build(ats, "https://x.invalid", &profile(), None).unwrap();
            for entry in &plan.entries {
                if let Action::SetText { labels, .. } = &entry.action {
                    for label in labels {
                        assert!(
                            !crate::flavor::label_is_demographic(label),
                            "{ats:?} emits {label:?}, which is the only kind of word \
                             that could fire a check reading Perch's own labels"
                        );
                    }
                }
            }
        }
        let script = script_for(Ats::Ashby);
        assert!(script.contains("function refusedBox("));
        assert!(
            script.contains("if (refusedBox(el)) { return null; }"),
            "the refusal reads the matched label rather than the box"
        );
    }

    #[test]
    fn a_box_is_judged_by_the_heading_it_sits_under_as_well_as_its_label() {
        // The US disability self-identification form asks for a Name, and
        // that label is exactly one Perch fills. Nothing in the label says
        // which form it belongs to. The heading above it does, so a box is
        // read together with the block it is printed in.
        let script = script_for(Ats::Ashby);
        assert!(script.contains("function heading("));
        assert!(script.contains("h1, h2, h3, h4, h5, h6, legend"));
        assert!(
            script.contains("return refused(heading(el));"),
            "the block a box sits in is not read"
        );
        // And the words such a block is titled with are on the list the page
        // checks, so the heading has something to be caught by.
        for word in ["self-identif", "disability", "veteran", "gender"] {
            assert!(
                script.contains(&format!("\"{word}\"")),
                "{word:?} is missing from the list the page checks"
            );
        }
    }

    #[test]
    fn a_label_whose_for_names_nothing_still_finds_the_box_beside_it() {
        // Ashby writes `for="_systemfield_location"` on its Location label
        // and renders a combobox with no id for it to name, so the lookup
        // ended at a null and the value the plan promised was typed nowhere.
        // A `for` that names nothing is not an answer, so the box the label
        // is printed with is used, and only when there is exactly one.
        let script = script_for(Ats::Ashby);
        assert!(script.contains("function boxFor("));
        assert!(
            !script.contains("? document.getElementById(tags[j].htmlFor)"),
            "a `for` naming nothing still ends the lookup"
        );
        assert!(script.contains("return near.length === 1 ? near[0] : null;"));

        // And Location is a box with both, so the fallback has a label to
        // work from when the selector finds nothing.
        let plan = plan::build(Ats::Ashby, "https://x.invalid", &profile(), None).unwrap();
        let location = plan
            .entries
            .iter()
            .find(|e| e.label == "Location")
            .expect("Ashby fills a location");
        match &location.action {
            Action::SetText {
                selector, labels, ..
            } => {
                assert!(!selector.is_empty());
                assert!(labels.contains(&"Location".to_string()), "{labels:?}");
            }
            other => panic!("Location is {other:?}"),
        }
    }

    #[test]
    fn a_value_perch_typed_is_not_put_back_over_what_the_person_changed_it_to() {
        // A planned value whose box is not on the form keeps the loop running
        // its full few seconds, and every pass used to put every value back.
        // A correction made in that window was silently undone, and the
        // person could send the profile value believing they had replaced it.
        let script = script_for(Ats::Ashby);
        assert!(
            !script.contains("if (el.value !== plan[i].v) { put(el, plan[i].v); waiting++; }"),
            "every pass puts the value back over whatever is in the box"
        );
        assert!(
            script.contains("if (wrote[i] && el.value !== '') { continue; }"),
            "a box holding different words is written over"
        );
        // A box the form emptied is still refilled, which is what the loop is
        // there for.
        assert!(script.contains("if (el.value === plan[i].v) { wrote[i] = true; continue; }"));
    }

    /// The harness that runs the emitted script the way the webview runs it:
    /// as an expression, whose value is the only thing the caller receives.
    ///
    /// `Page::Bare` gives it a page with none of its boxes on it, which
    /// answers nothing to every lookup exactly as an empty page does.
    /// `Page::None` takes the document away, so the script's own first lookup
    /// throws and the catch is the only thing that can answer.
    ///
    /// Later passes are turned off: the value under test is what came back,
    /// and a live timer would hold the process open for the settle window.
    const HARNESS: &str = r#"
const fs = require('fs');
globalThis.window = globalThis;
globalThis.setTimeout = () => 0;
if (process.argv[3] === 'bare') {
  globalThis.document = {
    querySelector: () => null,
    querySelectorAll: () => [],
    getElementById: () => null,
    body: { textContent: '' },
  };
}
process.stdout.write(String((0, eval)(fs.readFileSync(process.argv[2], 'utf8'))));
"#;

    enum Page {
        Bare,
        None,
    }

    fn run(script: &str, page: Page) -> String {
        node(
            HARNESS,
            script,
            match page {
                Page::Bare => "bare",
                Page::None => "none",
            },
        )
    }

    /// The emitted script, run under a harness, with whatever that harness
    /// takes as its page.
    fn node(harness: &str, script: &str, page: &str) -> String {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);

        let dir = std::env::temp_dir().join(format!(
            "perch-fill-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("harness.js"), harness).unwrap();
        std::fs::write(dir.join("fill.js"), script).unwrap();

        let out = std::process::Command::new("node")
            .arg(dir.join("harness.js"))
            .arg(dir.join("fill.js"))
            .arg(page)
            .output()
            .expect("these tests run the emitted script, which needs node on PATH");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            out.status.success(),
            "the script did not run to completion:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap()
    }

    /// The report as a person would read it back.
    fn report(raw: &str) -> serde_json::Value {
        serde_json::from_str(raw).unwrap_or_else(|e| panic!("{raw:?} is not a report: {e}"))
    }

    /// A page small enough to read and real enough to run the fill against.
    ///
    /// Its boxes answer to a selector and hold a value, and they can be
    /// emptied, built late, taken apart or made to reshape what they are
    /// given while the script is running. Time is a number the test moves,
    /// so a settle window measured in seconds can be watched pass in a test
    /// that takes none.
    ///
    /// What comes back is the value the script returned, the account read off
    /// the page afterwards the way Perch reads it, and what the boxes hold by
    /// then. The last of those is the point: a report is only worth as much
    /// as its agreement with the form it describes.
    const PAGE: &str = r#"
const fs = require('fs');
globalThis.window = globalThis;
const spec = JSON.parse(process.argv[3]);

let now = 0;
const clamp = spec.clamp || 0;
Date.now = () => now;
const timers = [];
globalThis.setTimeout = (fn, ms) => {
  timers.push({ fn, at: now + Math.max(ms || 0, clamp) });
  return timers.length;
};

globalThis.Event = class Event {
  constructor(type, init) {
    this.type = type;
    this.bubbles = !!(init && init.bubbles);
  }
};
globalThis.File = class File {
  constructor(parts, name, opts) {
    this.name = name;
    this.type = (opts && opts.type) || '';
  }
};
if (spec.dataTransfer !== false) {
  globalThis.DataTransfer = class DataTransfer {
    constructor() {
      const list = [];
      this.list = list;
      this.items = { add(f) { list.push(f); } };
    }
    get files() { return this.list; }
  };
}
globalThis.Node = { DOCUMENT_POSITION_FOLLOWING: 4 };

class Box {
  constructor(made) {
    this.sel = made.sel;
    this.tagName = made.tag || 'INPUT';
    this.type = made.type || 'text';
    this.id = made.id || '';
    this.shape = made.shape || null;
    this.held = '';
    this.heard = [];
    this.tellings = 0;
    if (this.type === 'file') { this.files = null; }
  }
  addEventListener(name, fn) { this.heard.push([name, fn]); }
  dispatchEvent(ev) {
    if (ev.type === 'change') { this.tellings++; }
    for (const [name, fn] of this.heard) {
      if (name === ev.type) { fn.call(this, ev); }
    }
    return true;
  }
}
// A box that prints back what it was given in its own shape, which is what an
// ordinary phone field does to a number.
function shaped(how, value) {
  if (how !== 'phone') { return value; }
  const digits = value.replace(/[^0-9]/g, '').slice(-10);
  return digits.length === 10
    ? '(' + digits.slice(0, 3) + ') ' + digits.slice(3, 6) + '-' + digits.slice(6)
    : value;
}
const value = {
  get() { return this.held; },
  set(v) { this.held = shaped(this.shape, String(v)); },
  configurable: true,
};
globalThis.HTMLInputElement = class HTMLInputElement extends Box {};
globalThis.HTMLTextAreaElement = class HTMLTextAreaElement extends Box {};
globalThis.HTMLSelectElement = class HTMLSelectElement extends Box {};
for (const kind of [HTMLInputElement, HTMLTextAreaElement, HTMLSelectElement]) {
  Object.defineProperty(kind.prototype, 'value', value);
}

const boxes = [];
function build(made) {
  const kind = made.tag === 'TEXTAREA'
    ? HTMLTextAreaElement
    : made.tag === 'SELECT' ? HTMLSelectElement : HTMLInputElement;
  boxes.push(new kind(made));
}
function find(sel) {
  return boxes.find((b) => b.sel === sel && !b.gone) || null;
}
function words() {
  let text = spec.body || '';
  if (spec.namesFiles) {
    for (const b of boxes) {
      for (const f of b.files || []) { text += ' ' + f.name; }
    }
  }
  return text;
}
globalThis.document = {
  querySelector: find,
  querySelectorAll: () => [],
  getElementById: (id) => boxes.find((b) => b.id === id && !b.gone) || null,
  body: { get textContent() { return words(); } },
};

function change(what) {
  for (const sel of what.empty || []) {
    const b = find(sel);
    if (b) { b.held = ''; }
  }
  for (const made of what.build || []) { build(made); }
  for (const sel of what.remove || []) {
    const b = find(sel);
    if (b) { b.gone = true; }
  }
  for (const sel of what.breakOpen || []) {
    const b = find(sel);
    if (b) {
      Object.defineProperty(b, 'held', {
        get() { throw new TypeError('the page took this box apart'); },
        configurable: true,
      });
    }
  }
  if (what.body !== undefined) { spec.body = what.body; }
}

for (const made of spec.boxes || []) { build(made); }
if (spec.owns) {
  Object.defineProperty(globalThis, '__perchFill', {
    value: spec.owns, writable: false, configurable: false,
  });
}

const answer = String((0, eval)(fs.readFileSync(process.argv[2], 'utf8')));
const events = (spec.events || []).slice().sort((a, b) => a.at - b.at);
const readAt = spec.readAt === undefined ? 7500 : spec.readAt;
const passes = [now];
let next = 0;
for (;;) {
  const dueTimer = timers.length ? Math.min(...timers.map((t) => t.at)) : Infinity;
  const dueEvent = next < events.length ? events[next].at : Infinity;
  const step = Math.min(dueTimer, dueEvent);
  if (!isFinite(step) || step > readAt) { break; }
  now = step;
  while (next < events.length && events[next].at <= now) { change(events[next]); next++; }
  if (dueTimer === step) {
    const i = timers.findIndex((t) => t.at === step);
    const timer = timers.splice(i, 1)[0];
    passes.push(now);
    timer.fn();
  }
}
now = readAt;
while (next < events.length && events[next].at <= now) { change(events[next]); next++; }

function held(b) {
  try { return b.value; } catch (err) { return null; }
}
process.stdout.write(JSON.stringify({
  first: answer,
  read: JSON.stringify(window.__perchFill || null),
  passes,
  values: boxes.map((b) => [b.sel, held(b)]),
  files: boxes.map((b) => [b.sel, (b.files || []).map((f) => f.name)]),
  tellings: boxes.map((b) => [b.sel, b.tellings]),
}));
"#;

    /// The script run against a page that changes under it. The page is
    /// written as JSON: the boxes it starts with, what happens to them and
    /// when, how slowly it runs a timer, and the moment Perch reads the
    /// account back.
    fn run_on(script: &str, page: &str) -> serde_json::Value {
        let raw = node(PAGE, script, page);
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{raw:?} is not a run: {e}"))
    }

    /// What a box holds at the end of a run.
    fn value_in(run: &serde_json::Value, selector: &str) -> String {
        paired(run, "values", selector)
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    /// The names of the files sitting on a box at the end of a run.
    fn names_on(run: &serde_json::Value, selector: &str) -> Vec<String> {
        paired(run, "files", selector)
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap().to_string())
            .collect()
    }

    /// How many times the form was told about a box after the file landed on
    /// it. The first telling is part of handing the file over.
    fn told_again(run: &serde_json::Value, selector: &str) -> u64 {
        paired(run, "tellings", selector)
            .as_u64()
            .unwrap()
            .saturating_sub(1)
    }

    fn paired(run: &serde_json::Value, of: &str, selector: &str) -> serde_json::Value {
        run[of]
            .as_array()
            .unwrap()
            .iter()
            .find(|pair| pair[0] == selector)
            .unwrap_or_else(|| panic!("{selector} is not a box in this run"))[1]
            .clone()
    }

    /// The five boxes a Greenhouse posting gives this profile, written as the
    /// page the fill meets.
    const GREENHOUSE_BOXES: &str = r#"[
        { "sel": "input#first_name", "id": "first_name" },
        { "sel": "input#last_name", "id": "last_name" },
        { "sel": "input#email", "id": "email" },
        { "sel": "input#phone", "id": "phone" },
        { "sel": "input#job_application_website", "id": "job_application_website" }
    ]"#;

    fn greenhouse_script(resume: Option<&str>) -> String {
        let plan = plan::build(
            Ats::Greenhouse,
            "https://boards.greenhouse.io/x/jobs/1",
            &profile(),
            resume,
        )
        .unwrap();
        let mut files = BTreeMap::new();
        for entry in &plan.entries {
            if let Action::AttachFile { selector, .. } = &entry.action {
                files.insert(
                    selector.clone(),
                    Attachment {
                        name: "cv.pdf".into(),
                        mime: "application/pdf".into(),
                        base64: "JVBERi0xLjQK".into(),
                    },
                );
            }
        }
        to_script(&plan, &files)
    }

    #[test]
    fn the_account_is_read_when_it_is_asked_for_and_not_when_the_loop_last_ran() {
        // The loop stops as soon as every value is in its box, which on a form
        // already in the page is the second pass. A framework mounting after
        // that empties the form, and a report written on pass two would still
        // be saying that five values are in it.
        let run = run_on(
            &greenhouse_script(None),
            &format!(
                r#"{{ "boxes": {GREENHOUSE_BOXES},
                      "events": [{{ "at": 2000, "empty": [
                        "input#first_name", "input#last_name", "input#email",
                        "input#phone", "input#job_application_website" ] }}] }}"#
            ),
        );
        assert_eq!(report(run["first"].as_str().unwrap())["filled"], 5);
        assert_eq!(value_in(&run, "input#email"), "");

        let said = report(run["read"].as_str().unwrap());
        assert_eq!(said["filled"], 0, "the account was taken before the read");
        assert_eq!(said["settled"], true);
    }

    #[test]
    fn a_box_that_reshapes_what_it_was_given_is_counted_as_filled() {
        // A phone box that prints (415) 555-0148 back is holding the number
        // it was handed. Compared letter for letter it read as an empty box,
        // and the person was told a value was short without being told which.
        let run = run_on(
            &greenhouse_script(None),
            r#"{ "boxes": [
                   { "sel": "input#first_name" }, { "sel": "input#last_name" },
                   { "sel": "input#email" },
                   { "sel": "input#phone", "shape": "phone" },
                   { "sel": "input#job_application_website" } ] }"#,
        );
        assert_eq!(value_in(&run, "input#phone"), "(415) 555-0148");

        let said = report(run["read"].as_str().unwrap());
        assert_eq!(said["filled"], 5, "{said}");
        assert_eq!(said["changed"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn a_box_holding_something_else_is_named_rather_than_only_counted() {
        // A count one short of the plan, with every box found, said that a
        // value was missing and gave the person no way to tell which.
        let run = run_on(
            &greenhouse_script(None),
            &format!(
                r#"{{ "boxes": {GREENHOUSE_BOXES},
                      "events": [{{ "at": 2000, "empty": ["input#email"] }}] }}"#
            ),
        );
        let said = report(run["read"].as_str().unwrap());
        assert_eq!(said["filled"], 4);
        assert_eq!(said["missing"].as_array().unwrap().len(), 0);
        assert_eq!(said["changed"][0], "Email", "{said}");
    }

    #[test]
    fn a_form_that_already_names_a_file_of_its_own_is_still_given_yours() {
        // The file was handed over only if the page did not already print its
        // name, so a board listing a résumé called cv.pdf kept the person's
        // own cv.pdf from ever reaching the box, and the account said the
        // form had the file.
        let run = run_on(
            &greenhouse_script(Some("/docs/cv.pdf")),
            r#"{ "boxes": [{ "sel": "input#resume", "type": "file" }],
                 "body": "Resume on file: cv.pdf" }"#,
        );
        assert_eq!(names_on(&run, "input#resume"), vec!["cv.pdf"]);

        let said = report(run["read"].as_str().unwrap());
        assert_eq!(said["file"], true);
        assert_eq!(said["fileGiven"], true, "{said}");
    }

    #[test]
    fn a_file_perch_could_not_hand_over_is_not_reported_as_one_it_did() {
        // No box to put it on, and a page saying the words anyway. What Perch
        // did is the part it can answer for.
        let run = run_on(
            &greenhouse_script(Some("/docs/cv.pdf")),
            r#"{ "boxes": [], "body": "cv.pdf is already on your profile" }"#,
        );
        let said = report(run["read"].as_str().unwrap());
        assert_eq!(said["file"], true);
        assert_eq!(said["fileGiven"], false, "{said}");
        assert_eq!(said["fileNamed"], true, "the page's own words are reported");
    }

    #[test]
    fn the_loop_is_over_before_the_account_is_read_however_slowly_the_page_runs_a_timer() {
        // A window behind another one has its timers slowed to one a second.
        // Counted in passes, the loop was still writing values into the form
        // long after the account had been read and shown to the person.
        let run = run_on(
            &greenhouse_script(None),
            &format!(r#"{{ "boxes": {GREENHOUSE_BOXES}, "clamp": 1000 }}"#),
        );
        let passes = run["passes"].as_array().unwrap();
        let last = passes.last().unwrap().as_f64().unwrap();
        assert!(last <= 6_000.0, "the last pass ran at {last}ms");
        assert_eq!(report(run["read"].as_str().unwrap())["settled"], true);
    }

    #[test]
    fn a_throw_after_the_values_landed_still_counts_the_boxes() {
        // The plan is typed before the file is attached, so an attachment that
        // throws stops a fill that has already filled the form. Saying nothing
        // was typed there is a claim about a page nobody read.
        let run = run_on(
            &greenhouse_script(Some("/docs/cv.pdf")),
            r#"{ "boxes": [
                   { "sel": "input#first_name" }, { "sel": "input#last_name" },
                   { "sel": "input#email" }, { "sel": "input#phone" },
                   { "sel": "input#job_application_website" },
                   { "sel": "input#resume", "type": "file" } ],
                 "dataTransfer": false }"#,
        );
        let said = report(run["first"].as_str().unwrap());
        assert_eq!(said["filled"], 5, "{said}");
        assert!(
            said["error"]
                .as_str()
                .expect("no error text came back")
                .contains("DataTransfer"),
            "{said}"
        );
        assert_eq!(value_in(&run, "input#email"), "dana@dferreira.dev");
    }

    #[test]
    fn a_pass_that_throws_says_so_rather_than_leaving_the_account_reading_as_settled() {
        // Every pass after the first runs from a timer of its own, where a
        // throw reached no catch at all: the loop stopped where it was and the
        // account it left behind said nothing had gone wrong.
        // The website box is not on this form, so the loop is still going
        // when the page takes the email box apart.
        let run = run_on(
            &greenhouse_script(None),
            r#"{ "boxes": [
                   { "sel": "input#first_name" }, { "sel": "input#last_name" },
                   { "sel": "input#email" }, { "sel": "input#phone" } ],
                 "events": [{ "at": 600, "breakOpen": ["input#email"] }] }"#,
        );
        let said = report(run["read"].as_str().unwrap());
        assert!(
            said["error"]
                .as_str()
                .expect("a pass threw and the account says nothing about it")
                .contains("took this box apart"),
            "{said}"
        );
        // And the passes left still ran, because one box the page took apart
        // is not the rest of the form.
        assert!(run["passes"].as_array().unwrap().len() > 4, "{run}");
    }

    #[test]
    fn a_page_that_already_owns_the_name_cannot_answer_in_the_fills_place() {
        // A page is free to write to window before the fill runs, and what it
        // writes cannot be moved afterwards. The account Perch wrote carries a
        // mark, and the one the page wrote does not, which is the whole of
        // what the far side has to tell them apart by.
        let run = run_on(
            &greenhouse_script(None),
            r#"{ "boxes": [], "owns": { "filled": 9, "planned": 9, "missing": [],
                 "file": true, "fileNamed": true, "error": null } }"#,
        );
        let first = report(run["first"].as_str().unwrap());
        assert_eq!(first["filled"], 0);
        assert!(
            first["stamp"]
                .as_str()
                .is_some_and(|s| s.starts_with("perch-")),
            "the fill's own account is not marked as its own: {first}"
        );

        let read = report(run["read"].as_str().unwrap());
        assert_eq!(read["filled"], 9, "the page's answer is what came back");
        assert!(
            read["stamp"].is_null(),
            "the page could mark its own answer"
        );
    }

    #[test]
    fn the_emitted_script_hands_back_a_report_rather_than_undefined() {
        // Run, not read. The script that typed nothing on every board last
        // week passed every test in this file, because the value it gave back
        // was never looked at: there was none.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let said = report(&run(&script_for(ats), Page::Bare));
            assert!(said["error"].is_null(), "{ats:?} reported {said}");
            assert_eq!(said["filled"], 0);
            assert!(said["planned"].as_u64().unwrap() > 0, "{ats:?}: {said}");
            assert_eq!(said["file"], true);
            assert_eq!(said["fileNamed"], false);
        }
    }

    #[test]
    fn a_body_that_throws_comes_back_as_text_rather_than_going_quiet() {
        // The evaluator swallows exceptions, so a script that threw would
        // otherwise leave an empty form and no word of why. Here the document
        // is gone, so the first lookup the script makes throws for real.
        let said = report(&run(&script_for(Ats::Ashby), Page::None));
        let reason = said["error"].as_str().expect("no error text came back");
        assert!(
            reason.contains("document"),
            "{reason:?} does not say what broke"
        );
        assert_eq!(said["filled"], 0);
    }

    #[test]
    fn the_report_names_a_planned_box_that_is_not_on_the_form() {
        // A count on its own says a box is missing. The label says which one,
        // so the sentence a person reads can name it.
        let plan = plan::build(Ats::Ashby, "https://x.invalid", &profile(), None).unwrap();
        let said = report(&run(&to_script(&plan, &BTreeMap::new()), Page::Bare));
        let missing: Vec<String> = said["missing"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l.as_str().unwrap().to_string())
            .collect();
        for entry in &plan.entries {
            assert!(
                missing.contains(&entry.label),
                "{:?} was planned and is not on the form, and the report does not name it",
                entry.label
            );
        }
        assert_eq!(said["planned"].as_u64().unwrap() as usize, missing.len());
    }

    #[test]
    fn no_demographic_question_can_be_named_in_a_report() {
        // The report names boxes by their label, which is a word the plan now
        // carries. A plan that never holds such a label cannot print one.
        for ats in [Ats::Greenhouse, Ats::Lever, Ats::Ashby] {
            let said = report(&run(&script_for(ats), Page::Bare));
            for label in said["missing"].as_array().unwrap() {
                let label = label.as_str().unwrap();
                assert!(
                    !crate::flavor::label_is_demographic(label),
                    "{ats:?} would print {label:?} in a report"
                );
            }
        }
    }

    #[test]
    fn an_attachment_with_no_file_behind_it_is_skipped() {
        let plan = plan::build(
            Ats::Lever,
            "https://x.invalid",
            &profile(),
            Some("/docs/cv.pdf"),
        )
        .unwrap();
        let script = to_script(&plan, &BTreeMap::new());
        // The helpers are always defined, and the loop that would call them is
        // always written. What decides whether anything is attached is the list
        // it reads, so that is what this asserts: no bytes, no entry. Testing
        // the shape of the loop instead only tested how it was formatted.
        assert!(script.contains("function attach("));
        assert!(
            !script.contains("b64: "),
            "a file the plan had no bytes for was written into the script anyway"
        );

        // And with the bytes present, the entry is there.
        let mut files = BTreeMap::new();
        for entry in &plan.entries {
            if let Action::AttachFile { selector, .. } = &entry.action {
                files.insert(
                    selector.clone(),
                    Attachment {
                        name: "cv.pdf".into(),
                        mime: "application/pdf".into(),
                        base64: "QQ==".into(),
                    },
                );
            }
        }
        assert!(to_script(&plan, &files).contains("b64: "));
    }
}
