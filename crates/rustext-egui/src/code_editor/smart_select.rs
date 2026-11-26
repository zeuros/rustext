//! Smart select (Shift+Alt+Right / Shift+Alt+Left): grows each selection to the syntax node
//! holding it (when the styler parses the text), else to the word, the line, then the whole
//! text; shrinking goes back through the selections it grew from.

use rustext_core::cursors::grown_range_without_syntax;
use rustext_core::{Command, Outcome, Selection, Selections};

use super::CodeEditor;

#[derive(Clone, Debug, Default)]
pub struct SmartSelectState {
    /// The selections each growth started from, the latest last.
    grown_from: Vec<Selections>,
    /// The selections the last growth or shrink left: other selections mean the user moved,
    /// and the history no longer applies.
    left: Option<Selections>,
}

impl CodeEditor<'_> {
    pub(super) fn grow_selections(&mut self, outcome: &mut Outcome) {
        let selections = self.state.selections().clone();
        if self.view.smart_select.left.as_ref() != Some(&selections) {
            self.view.smart_select.grown_from.clear();
        }
        let text = self.state.text().clone();
        let (styler, config) = (&mut self.styler, &self.state.config);
        let grown = selections.map_each(|selection| {
            let (start, end) = (selection.start(), selection.end());
            let node = styler.as_deref_mut().and_then(|styler| styler.enclosing_syntax_node(&text, start, end));
            node.or_else(|| grown_range_without_syntax(&text, config, start, end))
                .map_or(*selection, |(start, end)| Selection::new(start, end))
        });
        if grown == selections {
            return;
        }
        self.view.smart_select.grown_from.push(selections);
        self.set_smart_selections(grown, outcome);
    }

    pub(super) fn shrink_selections(&mut self, outcome: &mut Outcome) {
        if self.view.smart_select.left.as_ref() != Some(self.state.selections()) {
            return;
        }
        if let Some(previous) = self.view.smart_select.grown_from.pop() {
            self.set_smart_selections(previous, outcome);
        }
    }

    fn set_smart_selections(&mut self, selections: Selections, outcome: &mut Outcome) {
        let result = self.state.execute(Command::SetSelections(selections));
        *outcome = std::mem::take(outcome).merged(result);
        self.view.smart_select.left = Some(self.state.selections().clone());
    }
}
