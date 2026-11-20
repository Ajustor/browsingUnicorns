package io.github.ajustor.browsingunicorns

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build
import android.util.Log
import android.widget.Toast

/**
 * The outcome of an update's install session ([AppUpdates]): shows the
 * system's confirmation when it needs one, says why it failed otherwise.
 * On success the app is replaced and restarted: nothing to do.
 */
class UpdateInstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)
        when (status) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                val confirm = confirmIntent(intent) ?: return
                try {
                    context.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
                } catch (e: Exception) {
                    Log.w(TAG, "could not show the install confirmation", e)
                    report(context, context.getString(R.string.update_no_installer))
                }
            }
            PackageInstaller.STATUS_SUCCESS -> {}
            else -> {
                val detail = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)
                Log.w(TAG, "update install failed: status $status ($detail)")
                // Cancelled by the user: nothing to say, Install tries again.
                if (status == PackageInstaller.STATUS_FAILURE_ABORTED) return
                val reason = context.getString(
                    when (status) {
                        PackageInstaller.STATUS_FAILURE_BLOCKED -> R.string.update_install_blocked
                        PackageInstaller.STATUS_FAILURE_CONFLICT -> R.string.update_install_conflict
                        PackageInstaller.STATUS_FAILURE_INCOMPATIBLE -> R.string.update_install_incompatible
                        PackageInstaller.STATUS_FAILURE_INVALID -> R.string.update_install_invalid
                        PackageInstaller.STATUS_FAILURE_STORAGE -> R.string.update_install_storage
                        else -> R.string.update_install_error
                    },
                )
                report(context, context.getString(R.string.update_failed, reason))
            }
        }
    }

    @Suppress("DEPRECATION")
    private fun confirmIntent(intent: Intent): Intent? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
        } else {
            intent.getParcelableExtra(Intent.EXTRA_INTENT)
        }

    /** To the running app if there is one, as a toast otherwise. */
    private fun report(context: Context, message: String) {
        val updates = AppUpdates.live?.get()
        if (updates != null) {
            updates.installFailed(message)
        } else {
            Toast.makeText(context, message, Toast.LENGTH_LONG).show()
        }
    }

    private companion object {
        const val TAG = "UpdateInstallReceiver"
    }
}
