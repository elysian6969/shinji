use futures_util::stream::StreamExt;
use std::collections::HashMap;
use std::env;
use twilight_cache_inmemory::InMemoryCache;
use twilight_gateway::{Event, Intents, Shard};
use twilight_model::channel::message::AllowedMentions;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, UserMarker};
use twilight_model::id::Id;

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
type Result<T, E = Error> = std::result::Result<T, E>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let intents = Intents::DIRECT_MESSAGES
        | Intents::DIRECT_MESSAGE_REACTIONS
        | Intents::DIRECT_MESSAGE_TYPING
        | Intents::GUILDS
        | Intents::GUILD_BANS
        | Intents::GUILD_EMOJIS_AND_STICKERS
        | Intents::GUILD_INTEGRATIONS
        | Intents::GUILD_INVITES
        | Intents::GUILD_MEMBERS
        | Intents::GUILD_MESSAGES
        | Intents::GUILD_MESSAGE_REACTIONS
        | Intents::GUILD_MESSAGE_TYPING
        | Intents::GUILD_WEBHOOKS
        | Intents::GUILD_PRESENCES
        | Intents::GUILD_VOICE_STATES
        | Intents::MESSAGE_CONTENT;

    let token = env::var("DISCORD_TOKEN")?;
    let (shard, mut events) = Shard::new(token, intents);

    shard.start().await?;

    const GUILD_ID: Id<GuildMarker> = unsafe { Id::new_unchecked(249111029668249601) };
    const LOG_ID: Id<ChannelMarker> = unsafe { Id::new_unchecked(947749304339169310) };

    #[derive(Clone, Debug)]
    struct InviteData {
        code: String,
        uses: u64,
        inviter: Option<Id<UserMarker>>,
    }

    // fetch the current invite state
    let mut invites = {
        shard
            .config()
            .http_client()
            .guild_invites(GUILD_ID)
            .exec()
            .await?
            .model()
            .await?
            .into_iter()
            .map(|invite| {
                let code = invite.code;
                let data = InviteData {
                    code: code.clone(),
                    uses: invite.uses.unwrap_or(0),
                    inviter: invite.inviter.map(|user| user.id),
                };

                (code, data)
            })
            .collect::<HashMap<_, _>>()
    };

    let cache = InMemoryCache::builder().message_cache_size(500).build();

    while let Some(event) = events.next().await {
        cache.update(&event);

        match event {
            // update when someone creates an invite
            Event::InviteCreate(invite) => {
                let code = invite.code.clone();
                let data = InviteData {
                    code: code.clone(),
                    uses: 0,
                    inviter: invite.inviter.map(|user| user.id),
                };

                invites.insert(code, data);
            }
            // remove when an invite is removed, duh
            Event::InviteDelete(invite) => {
                invites.remove(&invite.code);
            }
            Event::MemberAdd(member) => {
                // fetch the updated invite state
                let current_invites = shard
                    .config()
                    .http_client()
                    .guild_invites(member.guild_id)
                    .exec()
                    .await?
                    .model()
                    .await?;

                let mut used = None;

                for invite in current_invites.into_iter() {
                    let code = invite.code;
                    let data = InviteData {
                        code: code.clone(),
                        uses: invite.uses.unwrap_or(0),
                        inviter: invite.inviter.map(|user| user.id),
                    };

                    if let Some(old_data) = invites.get(&code) {
                        if data.uses > old_data.uses {
                            used = Some(data.clone());
                        }
                    } else {
                        used = Some(data.clone());
                    }

                    invites.insert(code.clone(), data);
                }

                let user_id = member.user.id;

                if let Some(data) = used {
                    let code = data.code;
                    let used = data.uses;

                    let mut message = format!("<@{user_id}> joined via `discord.gg/{code}`");

                    if let Some(inviter) = data.inviter {
                        message.push_str(&format!(" by <@{inviter}>"));
                    }

                    message.push_str(&format!(" (used {used} times)"));

                    shard
                        .config()
                        .http_client()
                        .create_message(LOG_ID)
                        .allowed_mentions(Some(&AllowedMentions::builder().build()))
                        .content(&message)?
                        .exec()
                        .await?;
                } else {
                    if let Some(guild) = cache.guild(member.guild_id) {
                        if let Some(code) = guild.vanity_url_code() {
                            let message = format!(
                                "<@{user_id}> joined via `discord.gg/{code}` (vanity url code)"
                            );

                            shard
                                .config()
                                .http_client()
                                .create_message(LOG_ID)
                                .allowed_mentions(Some(&AllowedMentions::builder().build()))
                                .content(&message)?
                                .exec()
                                .await?;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}
