use std::str::FromStr;

use anyhow::{Context, Result};
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
        .get_async("/:service/:user_id/feed.xml", |_req, ctx| async move {
            let feed = KemonoFeed {
                service: ctx.param("service").unwrap().as_ref(),
                user_id: ctx.param("user_id").unwrap().as_ref(),
            };

            let out = render_feed(feed)
                .await
                .map_err(|e| worker::Error::RustError(e.to_string()))?;

            let mut headers = worker::Headers::new();
            headers.append("Content-Type", "application/atom+xml")?;

            Ok(worker::Response::from_body(worker::ResponseBody::Body(out))?.with_headers(headers))
        })
        .run(req, env)
        .await
}

pub async fn render_feed(feed: KemonoFeed<'_>) -> Result<Vec<u8>> {
    let posts = get_kemono_posts(feed).await?;
    let atom_feed = kemono_feed(feed, posts).await?;
    let out = atom_feed.write_to(vec![])?;
    Ok(out)
}

#[derive(Clone, Copy)]
pub struct KemonoFeed<'a> {
    pub service: &'a str,
    pub user_id: &'a str,
}

#[derive(Serialize, Deserialize)]
pub struct KemonoPost {
    pub id: String,
    pub title: String,
    pub content: String,
    pub published: NaiveDateTime,
}

static BASE_URL: &str = "https://kemono.su";
static BASE_IMG_URL: &str = "https://img.kemono.su";

pub fn kemono_entry(feed: KemonoFeed<'_>, post: KemonoPost) -> Entry {
    let dtm = post.published.and_utc();

    let link = format!(
        "{}/{}/user/{}/post/{}",
        BASE_URL, feed.service, feed.user_id, &post.id
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

pub async fn kemono_feed(
    feed: KemonoFeed<'_>,
    posts: impl IntoIterator<Item = KemonoPost>,
) -> Result<Feed> {
    let entries = posts
        .into_iter()
        .map(|post| kemono_entry(feed, post))
        .collect::<Vec<_>>();

    let name = get_kemono_name(feed).await?;

    let updated = entries
        .first()
        .map(|entry| entry.updated)
        .unwrap_or(FixedDateTime::from_str("1970-01-01T00:00:00Z").unwrap());

    let icon = format!("{}/icons/{}/{}", BASE_IMG_URL, feed.service, feed.user_id);

    Ok(FeedBuilder::default()
        .id(format!("kemono-feed/{}/{}", feed.service, feed.user_id))
        .title(format!("Posts of {} from {}", name, feed.service))
        .updated(updated)
        .authors(vec![PersonBuilder::default().name(name).build()])
        .links(vec![LinkBuilder::default()
            .href(kemono_link(feed))
            .rel("alternate".to_owned())
            .build()])
        .icon(Some(icon))
        .entries(entries)
        .build())
}

pub async fn get_kemono_posts(feed: KemonoFeed<'_>) -> Result<Vec<KemonoPost>> {
    Ok(reqwest::get(format!(
        "{}/api/v1/{}/user/{}",
        BASE_URL, feed.service, feed.user_id
    ))
    .await?
    .json()
    .await?)
}

fn kemono_link(feed: KemonoFeed) -> String {
    format!("{}/{}/user/{}", BASE_URL, feed.service, feed.user_id)
}

pub async fn get_kemono_name(feed: KemonoFeed<'_>) -> Result<String> {
    let page = reqwest::get(kemono_link(feed)).await?.text().await?;

    let html = Html::parse_document(&page);

    let name_span = html
        .select(&Selector::parse("span[itemprop=name]").unwrap())
        .next()
        .context("failed to find name")?;

    Ok(name_span.inner_html())
}
