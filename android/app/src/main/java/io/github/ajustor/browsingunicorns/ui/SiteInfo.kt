package io.github.ajustor.browsingunicorns.ui

import android.net.http.SslCertificate
import android.os.Build
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import java.security.MessageDigest
import java.text.DateFormat

/** What the padlock shows: the connection, and the certificate the site showed. */
@Composable
fun SiteInfoDialog(host: String, secure: Boolean, certificate: SslCertificate?, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onClick = onDismiss) { Text("Close") } },
        title = { Text(host) },
        text = {
            SelectionContainer {
                Column(Modifier.verticalScroll(rememberScrollState())) {
                    when {
                        !secure -> Text(
                            "Connection is not secure: what you send or see can be read on the way.",
                            color = MaterialTheme.colorScheme.error,
                        )
                        certificate == null -> Text("Connection is secure (HTTPS). Certificate not available yet.")
                        else -> {
                            Text("Connection is secure (HTTPS).")
                            Spacer(Modifier.height(12.dp))
                            Text("Certificate", style = MaterialTheme.typography.titleSmall)
                            CertificateRows(certificate)
                        }
                    }
                }
            }
        },
    )
}

@Composable
private fun CertificateRows(cert: SslCertificate) {
    val dates = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT)
    Row("Issued to", name(cert.issuedTo))
    Row("Issued by", name(cert.issuedBy))
    // The full certificate (names covered, fingerprint): Android 10+.
    val x509 = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) cert.x509Certificate else null
    x509?.subjectAlternativeNames
        ?.filter { it.getOrNull(0) == 2 } // dNSName
        ?.mapNotNull { it.getOrNull(1) as? String }
        ?.takeIf { it.isNotEmpty() }
        ?.let { Row("Covers", it.joinToString(", ")) }
    cert.validNotBeforeDate?.let { Row("Valid from", dates.format(it)) }
    cert.validNotAfterDate?.let { Row("Valid until", dates.format(it)) }
    x509?.let {
        Row("Serial number", it.serialNumber.toByteArray().hex())
        Row("SHA-256", MessageDigest.getInstance("SHA-256").digest(it.encoded).hex())
    }
}

@Composable
private fun Row(name: String, value: String) {
    Spacer(Modifier.height(6.dp))
    Text(name, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
    Text(value, style = MaterialTheme.typography.bodyMedium)
}

/** Common name and organization, as available. */
private fun name(dn: SslCertificate.DName): String {
    val cn = dn.cName.orEmpty()
    val o = dn.oName.orEmpty()
    return when {
        cn.isNotEmpty() && o.isNotEmpty() && o != cn -> "$cn ($o)"
        cn.isNotEmpty() -> cn
        o.isNotEmpty() -> o
        else -> dn.dName.orEmpty()
    }
}

private fun ByteArray.hex(): String = joinToString(":") { "%02X".format(it) }
