package cn.pointhub.desktop

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * 凭据仓的密钥托管：密钥生成后只存在 AndroidKeyStore 里，应用读不到明文。
 * Rust 通过 JNI 调这里做 AES-256-GCM 加解密，密文格式是「12 字节 IV + 密文」。
 *
 * 密钥会随卸载 / 清除数据一起消失，这是设计意图（见 DECISIONS 的 D05）；
 * 解不开时由 Rust 侧转成「请重新导入账号」，不阻断应用启动。
 */
object VaultKeystore {
    private const val ALIAS = "pointhub-vault"
    private const val PROVIDER = "AndroidKeyStore"
    private const val TRANSFORM = "AES/GCM/NoPadding"
    private const val IV_BYTES = 12
    private const val TAG_BITS = 128

    /** 加密，返回 IV + 密文；密钥不存在时自动生成。 */
    @JvmStatic
    fun seal(plain: ByteArray): ByteArray {
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val ct = cipher.doFinal(plain)
        return cipher.iv + ct
    }

    /** 解密。密钥丢失时抛异常，由 Rust 侧转成「请重新导入」的提示。 */
    @JvmStatic
    fun open(sealed: ByteArray): ByteArray {
        require(sealed.size > IV_BYTES) { "密文过短" }
        val iv = sealed.copyOfRange(0, IV_BYTES)
        val ct = sealed.copyOfRange(IV_BYTES, sealed.size)
        val cipher = Cipher.getInstance(TRANSFORM)
        cipher.init(
            Cipher.DECRYPT_MODE,
            loadKey() ?: throw IllegalStateException("系统密钥不存在"),
            GCMParameterSpec(TAG_BITS, iv),
        )
        return cipher.doFinal(ct)
    }

    private fun key(): SecretKey = loadKey() ?: generateKey()

    private fun loadKey(): SecretKey? {
        val store = KeyStore.getInstance(PROVIDER).apply { load(null) }
        return (store.getEntry(ALIAS, null) as? KeyStore.SecretKeyEntry)?.secretKey
    }

    private fun generateKey(): SecretKey {
        val gen = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        gen.init(
            KeyGenParameterSpec.Builder(
                ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .setRandomizedEncryptionRequired(true)
                .build()
        )
        return gen.generateKey()
    }
}
