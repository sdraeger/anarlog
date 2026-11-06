//! Names speakers from what is said in the meeting (English and German).
//!
//! - A speaker introducing themselves ("I'm Sarah", "ich bin Tom", "Sarah here")
//!   scores [`INTRODUCTION_WEIGHT`] for that participant.
//! - A name addressed at the start or end of a turn ("Sarah, what do you
//!   think?") scores [`ADDRESS_WEIGHT`] for the next speaker; a name thanked
//!   ("Thanks, Sarah") scores it for the previous one. Either takes
//!   [`ADDRESS_WEIGHT`] off the speaker who said it.
//!
//! Only names of the given candidates count, so a capitalized noun ("ich bin
//! Entwickler") or a stranger never names anyone, and every result is a known
//! person. A first name shared by two candidates is ambiguous and ignored unless
//! the full name is said. Names go out highest score first, each speaker and
//! person at most once, from [`THRESHOLD`] up: one introduction, or two
//! consistent address cues.

use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::sync::LazyLock;

use regex::Regex;

pub const INTRODUCTION_WEIGHT: f32 = 3.0;
pub const ADDRESS_WEIGHT: f32 = 1.0;
pub const THRESHOLD: f32 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct MentionTurn<K> {
    pub speaker: K,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MentionCandidate {
    pub human_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MentionNaming<K> {
    pub speaker: K,
    pub human_id: String,
    pub score: f32,
    /// Whether the speaker introduced themselves, as opposed to being addressed.
    pub introduced: bool,
}

/// Consecutive words of one speaker joined into turns, in the order given.
pub fn mention_turns<'a, K: Copy + PartialEq>(
    words: impl IntoIterator<Item = (K, &'a str)>,
) -> Vec<MentionTurn<K>> {
    let mut turns: Vec<MentionTurn<K>> = Vec::new();
    for (speaker, text) in words {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some(turn) if turn.speaker == speaker => {
                turn.text.push(' ');
                turn.text.push_str(text);
            }
            _ => turns.push(MentionTurn {
                speaker,
                text: text.to_string(),
            }),
        }
    }
    turns
}

pub fn name_speakers_from_mentions<K: Copy + Eq + Hash>(
    turns: &[MentionTurn<K>],
    candidates: &[MentionCandidate],
    unnamed: &HashSet<K>,
    taken: &HashSet<String>,
) -> Vec<MentionNaming<K>> {
    let names = CandidateNames::new(candidates, taken);
    let mut score: HashMap<(K, &str), f32> = HashMap::new();
    let mut introduced: HashSet<(K, &str)> = HashSet::new();

    for (index, turn) in turns.iter().enumerate() {
        let intros = names.find(&INTRODUCTION, &turn.text);
        let thanks = names.find(&THANKS, &turn.text);
        // "Thanks, Sarah." also reads as a name at the end of a turn; the
        // thanks points back, so it is not counted forward as well.
        let addressed: Vec<&str> = names
            .find(&ADDRESS, &turn.text)
            .into_iter()
            .filter(|human_id| !intros.contains(human_id) && !thanks.contains(human_id))
            .collect();

        for human_id in intros {
            *score.entry((turn.speaker, human_id)).or_default() += INTRODUCTION_WEIGHT;
            introduced.insert((turn.speaker, human_id));
        }
        let next = turns[index + 1..]
            .iter()
            .find(|other| other.speaker != turn.speaker);
        let previous = turns[..index]
            .iter()
            .rev()
            .find(|other| other.speaker != turn.speaker);
        let cues = addressed
            .into_iter()
            .map(|human_id| (human_id, next))
            .chain(thanks.into_iter().map(|human_id| (human_id, previous)));
        for (human_id, target) in cues {
            *score.entry((turn.speaker, human_id)).or_default() -= ADDRESS_WEIGHT;
            if let Some(target) = target {
                *score.entry((target.speaker, human_id)).or_default() += ADDRESS_WEIGHT;
            }
        }
    }

    // Ties go to the speaker heard first, so the result does not depend on hashing.
    let mut first_turn: HashMap<K, usize> = HashMap::new();
    for (index, turn) in turns.iter().enumerate() {
        first_turn.entry(turn.speaker).or_insert(index);
    }
    let mut ranked: Vec<_> = score
        .into_iter()
        .filter(|((speaker, _), score)| unnamed.contains(speaker) && *score >= THRESHOLD)
        .collect();
    ranked.sort_by(
        |((left_speaker, left_id), left), ((right_speaker, right_id), right)| {
            right
                .total_cmp(left)
                .then(first_turn[left_speaker].cmp(&first_turn[right_speaker]))
                .then(left_id.cmp(right_id))
        },
    );

    let mut named_speakers = HashSet::new();
    let mut named_people = HashSet::new();
    let mut namings = Vec::new();
    for ((speaker, human_id), score) in ranked {
        if !named_speakers.insert(speaker) {
            continue;
        }
        if !named_people.insert(human_id) {
            named_speakers.remove(&speaker);
            continue;
        }
        namings.push(MentionNaming {
            speaker,
            human_id: human_id.to_string(),
            score,
            introduced: introduced.contains(&(speaker, human_id)),
        });
    }
    namings
}

/// A name: one or two capitalized words.
const NAME: &str = r"(\p{Lu}[\p{L}\-]+(?:\s+\p{Lu}[\p{L}\-]+)?)";
const LEAD: &str = r"^\W*(?:(?i:hi|hey|hello|hallo|so|okay|ok|also|and|und|well|yes|ja|gut)\W+)?";

static INTRODUCTION: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    compile(&[
        &format!(r"\b(?i:my name is|my name's|i am|i'm|this is|it's|call me)\s+{NAME}"),
        &format!(
            r"\b(?i:ich bin|ich heiße|ich heisse|mein name ist|hier ist|hier spricht)\s+{NAME}"
        ),
        &format!(r"{LEAD}{NAME}\s+(?i:here|hier)\b"),
    ])
});
static ADDRESS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    compile(&[
        &format!(r"{LEAD}{NAME}\s*[,:]"),
        &format!(r",\s*{NAME}\s*[?!.]*\s*$"),
    ])
});
static THANKS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    compile(&[&format!(
        r"\b(?i:thanks|thank you|danke(?: schön| dir| ihnen)?|vielen dank)\W+{NAME}"
    )])
});

fn compile(patterns: &[&str]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|pattern| Regex::new(pattern).expect("name mention pattern"))
        .collect()
}

/// Lowercased full names and first names of the candidates, each to its person;
/// a first name two people share maps to nobody.
struct CandidateNames<'a> {
    by_name: HashMap<String, Option<&'a str>>,
}

impl<'a> CandidateNames<'a> {
    fn new(candidates: &'a [MentionCandidate], taken: &HashSet<String>) -> Self {
        let mut by_name: HashMap<String, Option<&'a str>> = HashMap::new();
        let mut add = |key: String, human_id: &'a str| {
            by_name
                .entry(key)
                .and_modify(|existing| {
                    if *existing != Some(human_id) {
                        *existing = None;
                    }
                })
                .or_insert(Some(human_id));
        };
        for candidate in candidates {
            if taken.contains(&candidate.human_id) {
                continue;
            }
            let full = candidate.name.trim().to_lowercase();
            if full.is_empty() {
                continue;
            }
            if let Some(first) = full.split_whitespace().next() {
                add(first.to_string(), &candidate.human_id);
            }
            add(full, &candidate.human_id);
        }
        Self { by_name }
    }

    /// The people the patterns name in `text`, in order, once each.
    fn find(&self, patterns: &[Regex], text: &str) -> Vec<&'a str> {
        let mut found = Vec::new();
        for pattern in patterns {
            for captures in pattern.captures_iter(text) {
                let Some(name) = captures.get(1) else {
                    continue;
                };
                // "Sarah's" names Sarah's thing, not Sarah.
                let rest = &text[name.end()..];
                if rest.starts_with("'s") || rest.starts_with("’s") {
                    continue;
                }
                let words: Vec<String> = name
                    .as_str()
                    .split_whitespace()
                    .map(str::to_lowercase)
                    .collect();
                let human_id = self
                    .lookup(&words.join(" "))
                    .or_else(|| words.first().and_then(|first| self.lookup(first)));
                if let Some(human_id) = human_id
                    && !found.contains(&human_id)
                {
                    found.push(human_id);
                }
            }
        }
        found
    }

    fn lookup(&self, key: &str) -> Option<&'a str> {
        self.by_name.get(key).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidates(names: &[(&str, &str)]) -> Vec<MentionCandidate> {
        names
            .iter()
            .map(|(human_id, name)| MentionCandidate {
                human_id: human_id.to_string(),
                name: name.to_string(),
            })
            .collect()
    }

    fn name(lines: &[(u8, &str)], people: &[(&str, &str)]) -> HashMap<u8, String> {
        let turns = mention_turns(lines.iter().map(|(speaker, text)| (*speaker, *text)));
        let unnamed = lines.iter().map(|(speaker, _)| *speaker).collect();
        name_speakers_from_mentions(&turns, &candidates(people), &unnamed, &HashSet::new())
            .into_iter()
            .map(|naming| (naming.speaker, naming.human_id))
            .collect()
    }

    const PEOPLE: &[(&str, &str)] = &[("h-sarah", "Sarah Miller"), ("h-tom", "Tom Becker")];

    #[test]
    fn self_introduction_names_the_speaker() {
        let names = name(
            &[(1, "Hi everyone, I'm Sarah from design."), (2, "Welcome.")],
            PEOPLE,
        );
        assert_eq!(names.get(&1).map(String::as_str), Some("h-sarah"));
        assert_eq!(names.get(&2), None, "a speaker nobody names stays unnamed");
    }

    #[test]
    fn german_introduction_names_the_speaker() {
        let names = name(
            &[(1, "Hallo zusammen, hier ist Tom."), (2, "Hallo Tom.")],
            PEOPLE,
        );
        assert_eq!(names.get(&1).map(String::as_str), Some("h-tom"));
    }

    #[test]
    fn a_noun_that_is_nobody_names_nobody() {
        assert!(name(&[(1, "Ich bin Entwickler im Team.")], PEOPLE).is_empty());
    }

    #[test]
    fn one_address_is_not_enough() {
        assert!(
            name(
                &[(1, "Sarah, what do you think?"), (2, "Fine by me.")],
                PEOPLE
            )
            .is_empty()
        );
    }

    #[test]
    fn repeated_address_names_the_next_speaker() {
        let names = name(
            &[
                (1, "Sarah, what do you think?"),
                (2, "Looks good."),
                (1, "Can you take the next item, Sarah?"),
                (2, "Sure."),
            ],
            PEOPLE,
        );
        assert_eq!(names.get(&2).map(String::as_str), Some("h-sarah"));
        assert_eq!(names.get(&1), None, "the one addressing is not Sarah");
    }

    #[test]
    fn thanks_point_back_to_the_previous_speaker() {
        let names = name(
            &[
                (2, "That's all from me."),
                (1, "Thanks, Tom."),
                (2, "One more thing."),
                (1, "Thank you Tom."),
            ],
            PEOPLE,
        );
        assert_eq!(names.get(&2).map(String::as_str), Some("h-tom"));
    }

    #[test]
    fn a_possessive_is_not_an_introduction() {
        assert!(name(&[(1, "This is Sarah's draft.")], PEOPLE).is_empty());
    }

    #[test]
    fn a_shared_first_name_needs_the_full_name() {
        let people = &[("h-sarah", "Sarah Miller"), ("h-sarah-k", "Sarah Klein")];
        assert!(name(&[(1, "I'm Sarah.")], people).is_empty());
        assert_eq!(
            name(&[(1, "I'm Sarah Klein.")], people)
                .get(&1)
                .map(String::as_str),
            Some("h-sarah-k"),
        );
    }

    #[test]
    fn taken_people_and_named_speakers_are_left_alone() {
        let turns = mention_turns([(1u8, "I'm Sarah."), (2, "I'm Tom.")]);
        let namings = name_speakers_from_mentions(
            &turns,
            &candidates(PEOPLE),
            &HashSet::from([2]),
            &HashSet::from(["h-tom".to_string()]),
        );
        assert!(namings.is_empty());
    }

    #[test]
    fn each_person_goes_to_one_speaker() {
        let names = name(&[(1, "I'm Sarah."), (2, "I'm Sarah too, funnily.")], PEOPLE);
        assert_eq!(names.values().filter(|id| *id == "h-sarah").count(), 1);
    }

    #[test]
    fn word_tokens_join_into_turns() {
        let turns = mention_turns([(1u8, " I'm"), (1, " Sarah"), (1, "."), (2, " Hi")]);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].text, "I'm Sarah .");
    }
}
