package io.github.ajustor.browsingunicorns

/** A section of a release's notes: its title (`Ajouté`...) and its items. */
data class NotesSection(val title: String, val items: List<String>)

/**
 * A release's notes (its CHANGELOG section, in Markdown) as sections of
 * items: `### Title` starts a section, `- ` an item, an indented line
 * goes on the item before it. Emphasis stars are dropped.
 */
fun releaseNotes(notes: String): List<NotesSection> {
    val sections = mutableListOf<NotesSection>()
    var title = ""
    var items = mutableListOf<String>()
    fun close() {
        if (items.isNotEmpty() || title.isNotEmpty()) sections += NotesSection(title, items)
        items = mutableListOf()
    }
    for (raw in notes.lines()) {
        val line = raw.trim()
        when {
            line.startsWith("#") -> {
                close()
                title = line.trimStart('#').trim()
            }
            line.startsWith("- ") || line.startsWith("* ") -> items += line.drop(2).trim()
            line.isNotEmpty() && items.isNotEmpty() -> items[items.lastIndex] = items.last() + " " + line
            line.isNotEmpty() -> items += line
        }
    }
    close()
    return sections
        .filter { it.items.isNotEmpty() }
        .map { s -> s.copy(items = s.items.map { it.replace("*", "") }) }
}
