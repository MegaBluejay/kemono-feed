use std::str::FromStr;

use anyhow::Result;
use atom_syndication::{
    Entry, EntryBuilder, Feed, FeedBuilder, FixedDateTime, LinkBuilder, PersonBuilder, Text,
};
use chrono::NaiveDateTime;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use worker::Router;

#[worker::event(fetch)]
pub async fn main(
    req: worker::Request,
    env: worker::Env,
    _ctx: worker::Context,
) -> worker::Result<worker::Response> {
    let router = Router::new();

    router
        .get_async("/feed.xml", |_req, _ctx| async move {
            let params = KemonoFeed {
                service: "patreon".to_owned(),
                user_id: "93878".to_owned(),
                username: "malinryden".to_owned(),
            };

            let posts = get_kemono_posts(&params)
                .await
                .map_err(|e| worker::Error::RustError(e.to_string()))?;

            let feed = kemono_feed(&params, posts);

            let out = feed
                .write_to(vec![])
                .map_err(|e| worker::Error::RustError(e.to_string()))?;

            let mut headers = worker::Headers::new();
            headers.append("Content-Type", "application/atom+xml")?;

            Ok(worker::Response::from_body(worker::ResponseBody::Body(out))?.with_headers(headers))
        })
        .run(req, env)
        .await
}

pub struct KemonoFeed {
    pub service: String,
    pub user_id: String,
    pub username: String,
}

#[derive(Serialize, Deserialize)]
pub struct KemonoPost {
    pub id: String,
    pub title: String,
    pub content: String,
    pub published: NaiveDateTime,
}

pub fn kemono_entry(feed: &KemonoFeed, post: KemonoPost) -> Entry {
    let dtm = post.published.and_utc();

    let link = format!(
        "https://kemono.su/{}/user/{}/post/{}",
        &feed.service, &feed.user_id, &post.id
    );

    let summary = Html::parse_fragment(&post.content)
        .select(&Selector::parse("p").unwrap())
        .next()
        .map(|para| Text::html(para.inner_html()));

    EntryBuilder::default()
        .id(post.id)
        .title(Text::plain(post.title))
        .updated(dtm)
        .links(vec![LinkBuilder::default()
            .href(link)
            .rel("alternate".to_owned())
            .build()])
        .summary(summary)
        .build()
}

pub fn kemono_feed(feed: &KemonoFeed, posts: impl IntoIterator<Item = KemonoPost>) -> Feed {
    let entries = posts
        .into_iter()
        .map(|post| kemono_entry(feed, post))
        .collect::<Vec<_>>();

    let updated = entries
        .first()
        .map(|entry| entry.updated)
        .unwrap_or(FixedDateTime::from_str("1970-01-01T00:00:00Z").unwrap());

    let link = format!("https://kemono.su/{}/user/{}", &feed.service, &feed.user_id);

    FeedBuilder::default()
        .id(format!("kemono-feed/{}/{}", &feed.service, &feed.user_id))
        .title(format!(
            "Posts of {} from {}",
            &feed.username, &feed.service
        ))
        .updated(updated)
        .authors(vec![PersonBuilder::default()
            .name(feed.username.clone())
            .build()])
        .links(vec![LinkBuilder::default()
            .href(link)
            .rel("alternate".to_owned())
            .build()])
        .entries(entries)
        .build()
}

pub async fn get_kemono_posts(feed: &KemonoFeed) -> Result<Vec<KemonoPost>> {
    Ok(reqwest::get(format!(
        "https://kemono.su/api/v1/{}/user/{}",
        &feed.service, &feed.user_id
    ))
    .await?
    .json()
    .await?)
}
