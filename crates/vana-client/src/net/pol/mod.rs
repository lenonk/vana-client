//! Signing in to PlayOnline in place of the Viewer: the chat service's key agreement, then the profile
//! service's member login and the game's session (the lobby's authCode and 16-byte value).
//! MODLOG "PlayOnline sign-in".

pub mod chat;
pub mod cipher;
pub mod polpro;

use super::lobby::Session;
use anyhow::{Context, Result, bail};
use polpro::{ReplyBody, Status};
use std::net::{Shutdown, SocketAddr, TcpStream};

/// The lobby FFXiMain's -net 3 path uses (`ffxi00.pol.com`, `-port 54001`).
pub const LOBBY_HOST: &str = "ffxi00.pol.com";
pub const LOBBY_PORT: u16 = 54001;

/// What the player types: the PlayOnline member and the Square Enix account it is registered to.
pub struct Credentials {
    pub pol_id: String,
    pub pol_password: String,
    pub se_id: String,
    pub se_password: String,
    pub otp: Option<String>,
}

/// Where the services are: polcore's host names, or a test server's (the profile host then fixed, not routed).
pub struct Servers {
    pub chat: (String, u16),
    pub profile: Option<SocketAddr>,
}

/// The profile service's side of a signed-in member: what polcore keeps between transactions.
pub struct Profile {
    cipher: cipher::PolBlowfish,
    member: polpro::Member,
    server: Option<SocketAddr>,
    status: Status,
    /// Each handle's 8 slot bytes (`character << 1 | 1`, 0x10405820), from the character list.
    slots: Vec<[u8; 8]>,
    /// PlayOnline's character list (1/3, 0x68-byte records).
    characters: Vec<Vec<u8>>,
}

impl Profile {
    fn transact(&mut self, category: u8, opcode: u8, payload: Option<&[u8]>, reply: ReplyBody) -> Result<Vec<u8>> {
        polpro::transact(&mut self.cipher, &mut self.member, &mut self.server, category, opcode, payload, reply)
    }

    /// ChangeMyStatus (4/5) with polcore's arguments; the reply's first 16 bytes are the session value.
    fn change_status(&mut self, args: [i32; 6]) -> Result<[u8; 16]> {
        let slots = &self.slots;
        let (body, status) =
            polpro::change_my_status(&self.status, args, |c| slots.get(c as usize).copied().unwrap_or_default());
        let r = self.transact(4, 5, Some(&body), ReplyBody::Fixed(0x20))?;
        self.status = status;
        Ok(r[..16].try_into().unwrap())
    }

    /// LoadMyStatus (4/6, 0x1001d490 state 6): the content and presence ([6] - 1, none: 4), the character from the
    /// handle's slot [2] when [1], the flag, activity and p5.
    fn load_status(&mut self) -> Result<()> {
        let s = self.transact(4, 6, Some(&[0; 0x14]), ReplyBody::Fixed(0x80))?;
        let st = &mut self.status;
        if s[6] == 0 {
            (st.content, st.presence) = (-1, 4);
        } else {
            if s[0] < 0x40 {
                st.content = s[0] as i32;
            }
            st.presence = s[6] as i32 - 1;
        }
        if s[1] == 0 || st.content < 0 {
            st.service = -1;
        } else if s[2] < 8 {
            st.service = (self.slots[st.content as usize][s[2] as usize] >> 1 & 0x3f) as i32;
        }
        st.flag = (s[3] == 1) as i32;
        st.activity = u16::from_le_bytes([s[4], s[5]]) as i32;
        st.p5 = s[7] as i32;
        Ok(())
    }

    /// LoadCharacterList (1/3, 0x100217a0): a linked record ([4]) fills its handle's ([5]) slot byte ([6]).
    fn load_characters(&mut self) -> Result<()> {
        let r = self.transact(1, 3, None, ReplyBody::Records(0x68))?;
        self.characters = r[8..].as_chunks::<0x68>().0.iter().take(r[0] as usize).map(|c| c.to_vec()).collect();
        for c in &self.characters {
            if c[4] & 1 != 0 && c[5] < 0x40 && c[6] < 8 && c[0] < 0x40 {
                self.slots[c[5] as usize][c[6] as usize] = c[0] << 1 | 1;
            }
        }
        Ok(())
    }

    /// At character select (FFXiMain step 3, subs 0xE..0x15): the PlayOnline entry with the lobby record's content id
    /// and world key, product 1, and +0x14 zero; a linked one becomes the active character `(its handle, its index,
    /// 0, -2, -1, 1)`.
    pub fn select(&mut self, content_id: u32, world_key: u32) -> Result<()> {
        let u32_at = |c: &[u8], o: usize| u32::from_le_bytes(c[o..o + 4].try_into().unwrap());
        let Some(c) = self.characters.iter().find(|c| {
            u16::from_le_bytes([c[8], c[9]]) == 1
                && u32_at(c, 0xc) == world_key
                && u32_at(c, 0x10) == content_id
                && u32_at(c, 0x14) == 0
        }) else {
            bail!("PlayOnline has no entry for this character.");
        };
        if c[4] & 1 == 0 {
            // FFXiMain would attach it to a free handle slot and store the list (1/10); not done here.
            bail!(
                "This character is not registered to a PlayOnline handle yet. Open it once in the PlayOnline Viewer."
            );
        }
        let (content, index) = ((c[5] & 0x3f) as i32, c[0] as i32);
        self.change_status([content, index, 0, -2, -1, 1])?;
        Ok(())
    }
}

/// A signed-in member: the chat link and the profile service.
pub struct Pol {
    chat: chat::Chat,
    pub profile: Profile,
}

impl Pol {
    /// The link the game keeps: the chat answers the server's PINGs until the session ends.
    pub fn into_link(self) -> Link {
        let sock = self.chat.socket();
        self.chat.keep_alive();
        Link { chat: sock, profile: Some(self.profile) }
    }
}

/// The PlayOnline session kept for the game. Dropping it does what FFXiMain does at logout, ChangeMyStatus
/// `(-2, -1, -2, -2, -1, -2)` (no active character, FUN_100ef680), then closes the chat (the Viewer sends no QUIT).
pub struct Link {
    chat: Option<TcpStream>,
    profile: Option<Profile>,
}

impl Drop for Link {
    fn drop(&mut self) {
        let (chat, profile) = (self.chat.take(), self.profile.take());
        std::thread::spawn(move || {
            if let Some(mut p) = profile
                && let Err(e) = p.change_status([-2, -1, -2, -2, -1, -2])
            {
                log::warn!("PlayOnline: clearing the active character: {e:#}");
            }
            if let Some(s) = chat {
                let _ = s.shutdown(Shutdown::Both);
            }
        });
    }
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("pol::Link")
    }
}

/// Signs the member in at polcore's hosts.
pub fn sign_in(c: &Credentials) -> Result<(Session, Pol)> {
    sign_in_at(c, &Servers { chat: (chat::HOST.to_string(), chat::PORT), profile: None })
}

/// Signs the member in as Viewer 1.18.15e and FFXiMain do: the chat login; polcore's connect, the member login (4/7)
/// and its ChangeMyStatus `(-2, -2, 1, -2, -1, -2)` (POLCON 0x1b, 0x28; both skipped when the chat said so);
/// app.dll's LoadMyStatus (4/6), handle and character lists (0/9, 1/3) and ChangeMyStatus `(handle, -2, 1, 2, 0, -2)`;
/// then FFXiMain's `(-2, -1, 0, -2, -1, 1)`, whose reply makes the lobby's session.
pub fn sign_in_at(c: &Credentials, servers: &Servers) -> Result<(Session, Pol)> {
    let chat = chat::Chat::sign_in(&servers.chat.0, servers.chat.1, &c.pol_id, &c.pol_password)?;
    log::info!("PlayOnline: chat signed in, profile host {}", polpro::host(chat.host_byte));
    let member = polpro::Member {
        pol_id: c.pol_id.clone(),
        pol_password: c.pol_password.clone(),
        addr20: chat.addr20,
        host_byte: chat.host_byte,
        fixed: servers.profile,
        residue: [0; 12],
    };
    // polcore's starting status (0x1007541c..: no handle or character, presence 4, activity 1000).
    let status = Status { content: -1, service: -1, flag: 0, presence: 4, p5: 0, activity: 1000, changed: false };
    let mut p = Profile {
        cipher: chat.cipher.clone(),
        member,
        server: None,
        status,
        slots: vec![[0; 8]; 64],
        characters: Vec::new(),
    };
    if !chat.skip_member_login {
        let secret = polpro::secret20(&c.se_password);
        let body = polpro::member_login(&c.se_id, &secret, c.otp.as_deref(), chat.now());
        p.transact(4, 7, Some(&body), ReplyBody::None).context("signing in the Square Enix account")?;
        p.change_status([-2, -2, 1, -2, -1, -2])?;
    }
    p.load_status()?;
    let handles = p.transact(0, 9, None, ReplyBody::Records(0x88))?;
    p.load_characters()?;
    // The handle the Viewer plays with: the current one, else the account's first (0/9 records: [0] handle index).
    let content = if p.status.content >= 0 {
        p.status.content
    } else if handles[0] > 0 {
        (handles[8] & 0x3f) as i32
    } else {
        bail!("This PlayOnline member has no handle.")
    };
    p.change_status([content, -2, 1, 2, 0, -2])?;
    let value = p.change_status([-2, -1, 0, -2, -1, 1])?;
    let auth = polpro::auth_code(&value, &p.member.addr20, chat.clock, &chat.key, p.member.host_byte);
    let mut auth_code = [0u8; 64];
    auth_code[..auth.len()].copy_from_slice(&auth);
    Ok((Session::PlayOnline { auth_code, k16: value }, Pol { chat, profile: p }))
}
