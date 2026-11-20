//! Texts built in Rust for the Android app (crates/mobile), errors apart;
//! it shares the others it needs (`sync::invalid_code`).

messages! {
    /// The home page typed is not an address nor a host name.
    fn not_a_web_address(input: &str) {
        en: "Not a web address: {input}",
        fr: "Ce n’est pas une adresse web\u{a0}: {input}",
    }

    /// A search engine name the core does not know.
    fn unknown_search_engine(name: &str) {
        en: "Unknown search engine: {name}",
        fr: "Moteur de recherche inconnu\u{a0}: {name}",
    }

    /// The extension install field holds neither a store link nor an id.
    fn paste_store_link_or_id() {
        en: "Paste a Chrome Web Store link or an extension id.",
        fr: "Collez un lien du Chrome Web Store ou l’identifiant d’une extension.",
    }

    /// An extension id from the app that is not one.
    fn not_an_extension_id() {
        en: "Not an extension id.",
        fr: "Ce n’est pas un identifiant d’extension.",
    }

    /// A user script id from the app that is not one.
    fn not_a_user_script_id() {
        en: "Not a user script id.",
        fr: "Ce n’est pas un identifiant de script utilisateur.",
    }
}
