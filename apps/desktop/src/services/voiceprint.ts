import { commands as transcriptionCommands } from "@anlg/plugin-transcription";

// Runs after a transcript is persisted and before audio retention may delete
// the recording — the embeddings must outlive the audio. Failures are logged
// and swallowed: losing candidates for one session is acceptable, blocking
// transcription completion is not.
export async function maybeExtractVoiceprintCandidates(input: {
  enabled: boolean;
  sessionId: string;
  transcriptId: string;
  audioPath: string | null | undefined;
}): Promise<void> {
  if (input.enabled && input.audioPath) {
    try {
      const result = await transcriptionCommands.extractVoiceprintCandidates(
        input.sessionId,
        input.transcriptId,
        input.audioPath,
      );
      if (result.status === "error") {
        console.error("[voiceprint] candidate extraction failed", result.error);
      }
    } catch (error) {
      console.error("[voiceprint] candidate extraction failed", error);
    }
  }

  // After the voiceprints, so it names only the speakers they left open. It
  // reads only the words, so it needs neither the recording nor remembered voices.
  try {
    const result = await transcriptionCommands.assignSpeakersFromMentions(
      input.sessionId,
      input.transcriptId,
    );
    if (result.status === "error") {
      console.error("[voiceprint] mention naming failed", result.error);
    }
  } catch (error) {
    console.error("[voiceprint] mention naming failed", error);
  }
}
