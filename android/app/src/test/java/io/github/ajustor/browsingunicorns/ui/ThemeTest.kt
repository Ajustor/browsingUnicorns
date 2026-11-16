package io.github.ajustor.browsingunicorns.ui

import androidx.compose.ui.graphics.Color
import org.junit.Assert.assertTrue
import org.junit.Test

class ThemeTest {
    private fun readable(text: Color, on: Color, min: Double) {
        val ratio = contrast(text, on)
        assertTrue("contrast $ratio < $min", ratio >= min)
    }

    @Test fun contrastOfKnownPairs() {
        assertTrue(Math.abs(contrast(Color.Black, Color.White) - 21.0) < 0.01)
    }

    @Test fun textIsReadableEverywhere() {
        readable(Unicorn.OnNight, Unicorn.Night, 4.5)
        readable(Unicorn.OnNight, Unicorn.NightDeep, 4.5)
        readable(Unicorn.DarkText, Unicorn.DarkBackground, 4.5)
        readable(Unicorn.DarkText, Unicorn.DarkSurface, 4.5)
        readable(Unicorn.LightText, Unicorn.LightBackground, 4.5)
        readable(Unicorn.LightText, Unicorn.LightSurface, 4.5)
        readable(Color.White, Unicorn.LightAccent, 4.5)
        // Header titles on the gradient's lightest end.
        readable(Color.White, Unicorn.GradientEnd, 3.0)
    }
}
