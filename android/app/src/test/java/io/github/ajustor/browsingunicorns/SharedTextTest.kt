package io.github.ajustor.browsingunicorns

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class SharedTextTest {
    @Test
    fun aSharedLinkIsOpened() {
        assertEquals("https://example.com/a?b=1", sharedInput("https://example.com/a?b=1"))
    }

    @Test
    fun theLinkInATitleAndLinkIsOpened() {
        assertEquals(
            "https://youtu.be/xyz",
            sharedInput("Regarde ça : Super vidéo https://youtu.be/xyz via l'app"),
        )
    }

    @Test
    fun theFirstLinkWins() {
        assertEquals("http://a.test/", sharedInput("http://a.test/ et https://b.test/"))
    }

    @Test
    fun aLinkEndsBeforeClosingPunctuation() {
        assertEquals("https://example.com/page", sharedInput("(voir https://example.com/page)."))
    }

    @Test
    fun textWithoutALinkIsSearched() {
        assertEquals("recette crêpes", sharedInput("  recette crêpes \n"))
    }

    @Test
    fun nothingSharedOpensNothing() {
        assertNull(sharedInput(null))
        assertNull(sharedInput("   "))
    }
}
