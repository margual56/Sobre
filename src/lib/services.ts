// Well-known senders and their brand marks, keyed by the registrable domain.
import {
  siAirbnb, siAliexpress, siAnthropic, siApple, siAtlassian, siBitbucket, siBitwarden, siBluesky,
  siBookingdotcom, siCloudflare, siCodeberg, siCoursera, siDhl, siDigitalocean, siDiscord, siDocker,
  siDropbox, siDuolingo, siEbay, siEpicgames, siEtsy, siFacebook, siFedex, siFigma, siGithub, siGitlab,
  siGlovo, siGodaddy, siGogdotcom, siGoogle, siHetzner, siHuggingface, siHumblebundle, siIberia, siIcloud,
  siIkea, siInstagram, siKaggle, siKickstarter, siLetsencrypt, siMastodon, siMedium, siMeta, siMozilla,
  siN26, siNamecheap, siNetflix, siNotion, siNpm, siOvh, siPatreon, siPaypal, siPinterest, siPlaystation,
  siProton, siPypi, siQuora, siReddit, siRevolut, siRyanair, siSentry, siShopify, siSignal, siSpotify,
  siStackoverflow, siSteam, siStrava, siStripe, siSubstack, siTailscale, siTelegram, siTiktok, siTrello,
  siTumblr, siTwitch, siUber, siUdemy, siUps, siVercel, siWhatsapp, siWikipedia, siWise, siX, siYoutube,
  siZara, siZoho, siZoom, type SimpleIcon,
} from "simple-icons";

const byDomain: Record<string, SimpleIcon> = {
  "gmail.com": siGoogle, "googlemail.com": siGoogle, "google.com": siGoogle, "youtube.com": siYoutube,
  "icloud.com": siIcloud, "me.com": siIcloud, "apple.com": siApple,
  "proton.me": siProton, "protonmail.com": siProton, "pm.me": siProton, "zoho.com": siZoho,
  "github.com": siGithub, "gitlab.com": siGitlab, "bitbucket.org": siBitbucket, "codeberg.org": siCodeberg,
  "atlassian.com": siAtlassian, "atlassian.net": siAtlassian, "trello.com": siTrello, "sentry.io": siSentry,
  "npmjs.com": siNpm, "pypi.org": siPypi, "docker.com": siDocker, "vercel.com": siVercel,
  "cloudflare.com": siCloudflare, "digitalocean.com": siDigitalocean, "hetzner.com": siHetzner,
  "ovh.com": siOvh, "ovhcloud.com": siOvh, "namecheap.com": siNamecheap, "godaddy.com": siGodaddy,
  "letsencrypt.org": siLetsencrypt, "tailscale.com": siTailscale, "bitwarden.com": siBitwarden,
  "stackoverflow.com": siStackoverflow, "stackoverflow.email": siStackoverflow, "figma.com": siFigma,
  "notion.so": siNotion, "notion.com": siNotion, "anthropic.com": siAnthropic, "huggingface.co": siHuggingface,
  "kaggle.com": siKaggle, "coursera.org": siCoursera, "udemy.com": siUdemy, "duolingo.com": siDuolingo,
  "mozilla.org": siMozilla, "mozilla.com": siMozilla, "wikipedia.org": siWikipedia, "wikimedia.org": siWikipedia,
  "paypal.com": siPaypal, "paypal.es": siPaypal, "stripe.com": siStripe, "revolut.com": siRevolut,
  "wise.com": siWise, "n26.com": siN26, "shopify.com": siShopify, "ebay.com": siEbay, "ebay.es": siEbay,
  "etsy.com": siEtsy, "aliexpress.com": siAliexpress, "ikea.com": siIkea, "zara.com": siZara,
  "netflix.com": siNetflix, "spotify.com": siSpotify, "twitch.tv": siTwitch, "steampowered.com": siSteam,
  "epicgames.com": siEpicgames, "gog.com": siGogdotcom, "humblebundle.com": siHumblebundle,
  "playstation.com": siPlaystation, "kickstarter.com": siKickstarter, "patreon.com": siPatreon,
  "x.com": siX, "twitter.com": siX, "facebook.com": siFacebook, "facebookmail.com": siFacebook,
  "instagram.com": siInstagram, "meta.com": siMeta, "whatsapp.com": siWhatsapp, "telegram.org": siTelegram,
  "signal.org": siSignal, "discord.com": siDiscord, "reddit.com": siReddit, "redditmail.com": siReddit,
  "pinterest.com": siPinterest, "tiktok.com": siTiktok, "tumblr.com": siTumblr, "quora.com": siQuora,
  "medium.com": siMedium, "substack.com": siSubstack, "bsky.app": siBluesky, "joinmastodon.org": siMastodon,
  "zoom.us": siZoom, "dropbox.com": siDropbox, "dropboxmail.com": siDropbox, "strava.com": siStrava,
  "airbnb.com": siAirbnb, "airbnb.es": siAirbnb, "uber.com": siUber, "booking.com": siBookingdotcom,
  "ryanair.com": siRyanair, "iberia.com": siIberia, "iberia.es": siIberia, "glovoapp.com": siGlovo,
  "dhl.com": siDhl, "dhl.es": siDhl, "fedex.com": siFedex, "ups.com": siUps,
};

const twoPartSuffix = new Set(["co", "com", "org", "net", "ac", "gov", "edu"]);

/** Last two labels of a host, or three for suffixes such as co.uk. */
export function registrable(host: string): string {
  const labels = host.toLowerCase().replace(/\.$/, "").split(".");
  const n = labels.length;
  if (n <= 2) return labels.join(".");
  const three = labels[n - 1].length === 2 && twoPartSuffix.has(labels[n - 2]);
  return labels.slice(three ? n - 3 : n - 2).join(".");
}

export function domainOf(addr: string): string {
  return addr.includes("@") ? addr.slice(addr.lastIndexOf("@") + 1).toLowerCase() : "";
}

export function serviceIcon(addr: string): SimpleIcon | null {
  const domain = domainOf(addr);
  return byDomain[domain] ?? byDomain[registrable(domain)] ?? null;
}
