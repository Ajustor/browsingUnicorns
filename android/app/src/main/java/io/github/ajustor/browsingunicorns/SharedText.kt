package io.github.ajustor.browsingunicorns

/** The first web link in a text, without the punctuation that may follow it. */
private val LINK = Regex("""https?://[^\s<>"«»]+""", RegexOption.IGNORE_CASE)

/**
 * What to open for text shared to the app (the share sheet): its first web
 * link (apps often share "title + link"), else the text itself, searched
 * like what is typed in the address bar; `null` when there is nothing.
 */
fun sharedInput(text: String?): String? {
    val trimmed = text?.trim()?.takeIf { it.isNotEmpty() } ?: return null
    val link = LINK.find(trimmed)?.value?.trimEnd('.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '’')
    return link ?: trimmed
}
