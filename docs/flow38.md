# flow38 — M38: architecture & runtime flows (Full-disk encryption + secrets)

> Derived from `docs/idea.md` §12 and the work38 plan.

## Component map

```
        userspace
        +-------------------+  +-----------------------+
        |  willocrypt CLI   |  |  willoc-secrets       |
        |  (luks bind etc.) |  |  (SecretService)      |
        +---------+---------+  +-----------+-----------+
                  |                        |
                  v                        v
        +---------+------------+  +--------+----------+
        |   kernel keyring     |  |  D-Bus org.fdo.   |
        |   (add_key, request) |  |  secrets          |
        +----------+-----------+  +-------------------+
                   |
                   v
        +-------------------------------+
        |  kernel crypt block layer     |
        |  AES-XTS, LUKS2 decode        |
        +----+-------------+------------+
             |             |
             v             v
        +--------+   +-----------+
        |  TPM   |   | block dev |
        | driver |   | (AHCI/NVMe)|
        +---+----+   +-----------+
            |
            v
         TPM 2.0 hw / swtpm
```

## Boot unlock (TPM, clean state)

```
bootloader:
  read LUKS2 header from /dev/sda1
  find token "tpm" (PCR list, sealed blob)
  invoke TPM Unseal:
    LoadKey(seal_priv, seal_pub)
    PolicyPCR(pcr_list)
    Unseal -> seal_secret
  seal_secret = master_key_for_slot N
  feed seal_secret to kernel cmdline as "luks.key=N"
kernel:
  crypt block dev opens with that key
  WilloFS mounts root
```

## Boot unlock (PCR mismatch)

```
TPM Unseal -> error TPM_RC_POLICY_FAIL (PCR_VALUE)
bootloader:
  fall back to passphrase prompt
  user types passphrase
  kernel Argon2id derives slot key
  if matches: open
  else: deny + retry (3 attempts)
notify: "secure-boot state changed; rebind via willocrypt to restore TPM unlock"
```

## Per-user `/home` unlock at login

```
PAM (M37): pam_open_session
  -> pam_keyring (or pam_unix passthrough):
       wraps user password as KDF input
       unlocks user's encrypted home subvolume key
       crypt block layer mounts /home/$user
  on logout: re-locks (overwrite key in keyring)
```

## willocrypt bind tpm

```
willocrypt luks bind /dev/sda1 tpm --pcr 0,7
  read existing master key (using passphrase)
  generate random 32-byte secret
  TPM:
    Create(template=sealed, sensitive={data: secret, policy: PCR_0_7_hash})
    Load -> ctx
    Unseal (test) -> ok
  add new LUKS2 token "tpm" with sealed blob + PCR list
  add new key slot keyed by `secret`
  write back header
```

## SecretService example

```
app -> D-Bus: org.freedesktop.secrets.CreateCollection("default", master_key=user-pwd-derived)
app -> CreateItem(collection, label="github_token", value=bytes, attributes={app:"git"})
app -> SearchItems({app:"git"}) -> list of items
app -> GetSecrets([item]) -> bytes (only if collection unlocked)

at user login:
  willoc-secrets unlocks "default" collection using user password
at user logout / lock:
  re-locks: derives nothing further; existing items stay encrypted on disk
```

## Argon2id slot derive

```
slot.salt = 16 random bytes
slot.params = { m=64MiB, t=3, p=4 }
key_material = argon2id(passphrase, slot.salt, m=64MiB, t=3, p=4, len=64)
slot_key = key_material[..32]
iv = key_material[32..48]
slot_blob_decrypt(AES-CBC, slot_key, iv) -> AF_split → MK
```

## Failure paths

- **Wrong passphrase** → AF-merge yields wrong MK; AES-XTS first sector decode mismatches LUKS magic; reject.
- **TPM busy/unavailable** → fall back to passphrase prompt.
- **PCR change** (kernel update) → TPM_RC_POLICY_FAIL → passphrase fallback + warning; surface in §36 settings.
- **Header backup missing + corruption** → unrecoverable; warn user at every install to back up header.
- **AES-NI absent** → throughput crashes; install warns; user can disable encryption (`--no-encrypt`).
- **Keyring full** → eviction policy LRU; sensitive-marked keys never evicted unless explicit.

## Data structures

```rust
pub struct Luks2Header {
    pub magic: [u8; 6],                   // "LUKS\xba\xbe"
    pub version: u16,                     // 2
    pub hdr_size: u64,
    pub seq_id: u64,
    pub label: [u8; 48],
    pub csum_alg: SmolStr,                // "sha256"
    pub salt: [u8; 64],
    pub uuid: [u8; 40],
    pub subsystem: [u8; 48],
    pub hdr_offset: u64,
    pub csum: [u8; 64],
    pub json_area: Vec<u8>,               // JSON metadata: keyslots, segments, digests, tokens
}

pub struct KeySlot {
    pub kdf: KdfParams,                   // argon2id { m, t, p, salt }
    pub key_size: u32,                    // 64 (XTS-512)
    pub af: AntiForensicSplit,
    pub area: SegmentRef,
}

pub trait BlockDevice {
    fn read(&self, lba: u64, buf: &mut [u8]) -> Result<(), Errno>;
    fn write(&self, lba: u64, buf: &[u8]) -> Result<(), Errno>;
}

pub struct CryptBlock<B: BlockDevice> {
    pub inner: B,
    pub xts: AesXtsCtx,
    pub sector_size: u32,
}

pub struct TpmCtx {
    pub iface: TpmInterface,              // TIS | CRB
    pub locality: u8,
}

pub struct KeyringEntry {
    pub id: KeyId,
    pub kind: KeyKind,                    // User | Logon | Asymmetric | Encrypted
    pub flags: KeyFlags,                  // Sensitive (never log)
    pub bytes: SecretBytes,
}
```
