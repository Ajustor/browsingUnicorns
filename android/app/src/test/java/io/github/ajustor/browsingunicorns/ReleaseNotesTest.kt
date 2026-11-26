package io.github.ajustor.browsingunicorns

import org.junit.Assert.assertEquals
import org.junit.Test

class ReleaseNotesTest {
    @Test
    fun sectionsAndTheirItemsAreRead() {
        val notes = """
            ### Ajouté

            - Les onglets peuvent s'afficher
              dans une colonne à gauche.
            - Un écran scindé.

            ### Corrigé

            - Le menu restait ouvert.
        """.trimIndent()
        assertEquals(
            listOf(
                NotesSection("Ajouté", listOf("Les onglets peuvent s'afficher dans une colonne à gauche.", "Un écran scindé.")),
                NotesSection("Corrigé", listOf("Le menu restait ouvert.")),
            ),
            releaseNotes(notes),
        )
    }

    @Test
    fun itemsWithoutAHeadingAndMarkdownAreKeptReadable() {
        assertEquals(
            listOf(NotesSection("", listOf("Un réglage dans Réglages › Onglets, ou `menu`."))),
            releaseNotes("- Un réglage dans *Réglages › Onglets*, ou `menu`."),
        )
    }

    @Test
    fun emptyNotesHaveNoSection() {
        assertEquals(emptyList<NotesSection>(), releaseNotes("  \n"))
    }
}
