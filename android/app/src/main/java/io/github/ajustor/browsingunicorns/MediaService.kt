package io.github.ajustor.browsingunicorns

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.util.Log

/**
 * Keeps the app running while a tab plays (music, a podcast, a video's
 * sound) with the app off screen, as other browsers do: a notification says
 * so, with the page's title and a button to pause. Started while a tab
 * plays, the app on screen (Android refuses it from the background), and
 * stopped when nothing plays any more.
 */
class MediaService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_PAUSE) {
            onPause()
            return START_NOT_STICKY
        }
        val title = intent?.getStringExtra(EXTRA_TITLE).orEmpty()
        val notification = notification(title)
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(NOTIFICATION_ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK)
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (e: Exception) {
            // Not allowed now (from the background): the app plays on as long as Android lets it.
            Log.w(TAG, "foreground refused", e)
            stopSelf()
        }
        // Killed by Android: not started again (the page is gone with it).
        return START_NOT_STICKY
    }

    private fun notification(title: String): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, getString(R.string.media_channel), NotificationManager.IMPORTANCE_LOW),
        )
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val pause = PendingIntent.getService(
            this,
            1,
            Intent(this, MediaService::class.java).setAction(ACTION_PAUSE),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return Notification.Builder(this, CHANNEL)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(title.ifBlank { getString(R.string.media_playing) })
            .setContentText(getString(R.string.media_playing))
            .setContentIntent(open)
            .setOngoing(true)
            .setShowWhen(false)
            .addAction(
                Notification.Action.Builder(
                    android.graphics.drawable.Icon.createWithResource(this, android.R.drawable.ic_media_pause),
                    getString(R.string.media_pause),
                    pause,
                ).build(),
            )
            .build()
    }

    companion object {
        private const val TAG = "MediaService"
        private const val CHANNEL = "media"
        private const val NOTIFICATION_ID = 7
        private const val ACTION_PAUSE = "pause"
        private const val EXTRA_TITLE = "title"

        /** What the notification's pause button does (WebViews sets it). */
        var onPause: () -> Unit = {}

        /** Shows (or updates) the notification of a tab playing [title]. */
        fun start(context: Context, title: String?) {
            val intent = Intent(context, MediaService::class.java).putExtra(EXTRA_TITLE, title)
            try {
                context.startForegroundService(intent)
            } catch (e: Exception) {
                // From the background (Android 12+): refused.
                Log.w(TAG, "start refused", e)
            }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, MediaService::class.java))
        }
    }
}
