//! Module containing all the parts for creating/encoding Mails.
//!


use std::{
    ops::Deref,
    fmt,
    mem
};

use soft_ascii_string::SoftAsciiString;
use futures::{
    future,
    Future,
    Async,
    Poll
};
use media_type::BOUNDARY;

use common::{
    MailType,
    encoder::EncodingBuffer
};
use headers::{
    Header, HeaderKind,
    HeaderMap,
    headers::{
        ContentType, _From,
        ContentTransferEncoding,
        Date, MessageId,
        ContentDisposition
    },
    header_components::{
        DateTime,
        MediaType
    },
    error::{
        HeaderValidationError,
    }
};

use ::mime::create_random_boundary;
use ::error::{MailError, OtherValidationError};
use ::context::Context;
use ::resource::*;

/// A type representing a Mail.
///
/// This type is used to represent a mail including headers and body.
/// It is also used for the bodies of multipart mime mail bodies as
/// they can be seen as "sub-mails" or "hirachical nested mails", at
/// last wrt. everything relevant on this type.
///
/// A mail can be created using the `Builder` or more specific either
/// the `SinglepartBuilder` or the `MultipartBuilder` for a multipart
/// mime mail.
///
/// # Example
///
/// This will create, encode and print a simple plain text mail.
///
/// ```
/// # extern crate futures;
/// # extern crate mail_base;
/// # extern crate mail_common;
/// # #[macro_use] extern crate mail_headers as headers;
/// # use futures::Future;
/// # use mail_common::MailType;
/// use std::str;
/// // either from `mail::headers` or from `mail_header as headers`
/// use headers::{
///     headers::*,
///     header_components::Domain
/// };
/// use mail_base::{
///     Mail, Resource,
///     default_impl::simple_context
/// };
///
/// # fn main() {
/// // Domain will implement `from_str` in the future,
/// // currently it doesn't have a validator/parser.
/// let domain = Domain::from_unchecked("example.com".to_owned());
/// // Normally you create this _once per application_.
/// let ctx = simple_context::new(domain, "xqi93".parse().unwrap())
///     .unwrap();
///
/// let mut mail = Mail::plain_text("Hy there!");
/// mail.insert_headers(headers! {
///     _From: [("I'm Awesome", "bla@examle.com")],
///     _To: ["unknow@example.com"],
///     Subject: "Hy there message"
/// }.unwrap());
///
/// // We don't added anythink which needs loading but we could have
/// // and all of it would have been loaded concurrent and async.
/// let encoded = mail.into_encodeable_mail(ctx.clone())
///     .wait().unwrap()
///     .encode_into_bytes(MailType::Ascii).unwrap();
///
/// let mail_str = str::from_utf8(&encoded).unwrap();
/// println!("{}", mail_str);
/// # }
/// ```
///
/// And here is an example to create the same mail using the
/// builder:
///
/// ```
/// # extern crate mail_base;
/// # #[macro_use] extern crate mail_headers as headers;
/// // either from `mail::headers` or from `mail_header as headers`
/// use headers::{
///     headers::*
/// };
/// use mail_base::{Mail,  Resource};
///
/// # fn main() {
/// let resource = Resource::sourceless_from_string("Hy there!");
/// let mut mail = Mail::new_singlepart_mail(resource);
/// mail.insert_headers(headers! {
///     _From: [("I'm Awesome", "bla@examle.com")],
///     _To: ["unknow@example.com"],
///     Subject: "Hy there message"
/// }.unwrap());
/// # }
/// ```
///
/// And here is an example creating a multipart mail
/// with a made up `multipart` type.
///
/// ```
/// # extern crate mail_base;
/// # #[macro_use] extern crate mail_headers as headers;
/// // either from `mail::headers` or from `mail_header as headers`
/// use headers::{
///     headers::*,
///     header_components::MediaType
/// };
/// use mail_base::{Mail, Resource};
///
/// # fn main() {
/// let sub_body1 = Mail::plain_text("Body 1");
/// let sub_body2 = Mail::plain_text("Body 2, yay");
///
/// // This will generate `multipart/x.made-up-think; boundary=randome_generate_boundary`
/// let media_type = MediaType::new("multipart", "x.made-up-thing").unwrap();
/// let mut mail = Mail::new_multipart_mail(media_type, vec![sub_body1, sub_body2]);
/// mail.insert_headers(headers! {
///     _From: [("I'm Awesome", "bla@examle.com")],
///     _To: ["unknow@example.com"],
///     Subject: "Hy there message"
/// }.unwrap());
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct Mail {
    headers: HeaderMap,
    body: MailBody,
}

/// A type which either represents a single body, or multiple modies.
///
/// Note that you could have a mime multipart body just containing a
/// single body _and_ it being semantically important to be this way,
/// so we have to differ between both kinds (instead of just having
/// a `Vec` of mails)
#[derive(Clone, Debug)]
pub enum MailBody {
    SingleBody {
        body: Resource
    },
    MultipleBodies {
        //TODO[now]: use Vec1
        bodies: Vec<Mail>,
        /// This is part of the standard! But we won't
        /// make it public available for now. Through
        /// there is a chance that we need to do so
        /// in the future as some mechanisms might
        /// misuse this, well unusual think.
        hidden_text: SoftAsciiString
    }
}

impl Mail {

    /// Create a new plain text mail.
    ///
    /// This will
    ///
    /// - turn the `text` into a `String`
    /// - create a `Resource` from the `String`
    ///   (with content type `text/plain; charset=utf-8`)
    /// - create a mail from the resource
    ///
    pub fn plain_text(text: impl Into<String>) -> Self {
        let resource = Resource::sourceless_from_string(text);
        Mail::new_singlepart_mail(resource)
    }

    /// Returns true if the body of the mail is a multipart body.
    pub fn has_multipart_body(&self) -> bool {
        self.body.is_multipart()
    }

    /// Create a new multipart mail with given content type and given bodies.
    ///
    /// Note that while the given `content_type` has to be a `multipart` content
    /// type (when encoding the mail) it is not required nor expected to have the
    /// boundary parameter. The boundary will always be automatically generated
    /// independently of wether or not it was passed as media type.
    pub fn new_multipart_mail(content_type: MediaType, bodies: Vec<Mail>) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(ContentType::body(content_type));
        Mail {
            headers,
            body: MailBody::MultipleBodies {
                bodies,
                hidden_text: SoftAsciiString::new()
            }
        }
    }

    /// Create a new non-multipart mail for given `Resource` as body.
    pub fn new_singlepart_mail(body: Resource) -> Self {
        let headers = HeaderMap::new();
        Mail {
            headers,
            body: MailBody::SingleBody { body }
        }
    }


    /// Inserts a new header into the header map.
    ///
    /// This will call `insert` on the inner `HeaderMap`,
    /// which means all behavior of `HeaderMap::insert`
    /// does apply, like e.g. the "max one" behavior.
    pub fn insert_header<H>(&mut self, header: Header<H>)
        where H: HeaderKind
    {
        self.headers_mut().insert(header);
    }

    /// Inserts all headers into the inner header map.
    ///
    /// This will call `HeaderMap::insert_all` internally
    /// which means all behavior of `HeaderMap::insert`
    /// does apply, like e.g. the "max one" behavior.
    pub fn insert_headers(&mut self, headers: HeaderMap) {
        self.headers_mut().insert_all(headers);
    }

    /// Returns a reference to the currently set headers.
    ///
    /// Note that some headers namely `Content-Transfer-Encoding` as well
    /// as `Content-Type` for singlepart mails are derived from the content
    /// and _should not_ be set. If done so they are either ignored or an
    /// error is caused by them in other parts of the crate (like e.g. encoding).
    /// Also `Date` is auto-generated if not set and it is needed.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Return a mutable reference to the currently set headers.
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Returns a reference to the body/bodies.
    pub fn body(&self) -> &MailBody {
        &self.body
    }

    /// Return a mutable reference to the body/bodies.
    pub fn body_mut(&mut self) -> &mut MailBody {
        &mut self.body
    }

    /// Validate the mail.
    ///
    /// This will mainly validate the mail headers by
    ///
    /// - checking if no ContentTransferHeader is given
    /// - (for mails with multipart bodies) checking if the content type
    ///   is a `multipart` media type
    /// - (for mail with non-multipart bodies) check if there is _no_
    ///   content type header (as the content type header will be derived
    ///   from he `Resource`)
    /// - running all header validators (with `use_contextual_validators`) this
    ///   also checks for "max one" consistency (see `HeaderMap`'s documentation
    ///   for more details)
    /// - doing this recursively with all contained mails
    ///
    /// Note that this will be called by `into_encodeable_mail`, therefor
    /// it is normally not required to call this function by yourself.
    ///
    /// **Be aware that this does a general validation applicable to both the
    /// top level headers and headers from multipart mail sub bodies.** This
    /// means it e.g. doesn't check if there are any of the required headers
    /// (`Date` and `From`).
    pub fn generally_validate_mail(&self) -> Result<(), MailError> {
        if self.has_multipart_body() {
            validate_multipart_headermap(self.headers())?;
        } else {
            validate_singlepart_headermap(self.headers())?;
        }
        match self.body() {
            &MailBody::SingleBody { .. } => {},
            &MailBody::MultipleBodies { ref bodies, .. } => {
                for body in bodies {
                    body.generally_validate_mail()?;
                }
            }
        }
        Ok(())
    }

    /// Turns the mail into a future with resolves to an `EncodeableMail`.
    ///
    /// While this future resolves it will do following thinks:
    ///
    /// 1. Validate the mail.
    ///    - This uses `generally_validate_mail`.
    ///    - Additionally it does check for required top level headers
    ///      which will not be auto-generated (the `From` header).
    ///
    /// 2. make sure all contained `Resources` are loaded
    ///    so that they can be used directly when encoding
    ///    - Note that all resources are loaded concurrently, or at last
    ///      the futures driving the loading are resolved concurrently.
    ///
    /// 3. insert all auto generated headers (like e.g. `Date` or
    ///    `ContentTransferEncoding`)
    ///
    /// Use this if you want to encode a mail. This is needed as `Resource`
    /// instances used in the mail are loaded "on-demand", i.e. if you attach
    /// two images but never turn the mail into an encodable mail the images
    /// are never loaded from disk.
    ///
    pub fn into_encodeable_mail<C: Context>(self, ctx: C) -> MailFuture<C> {
        MailFuture::new(self, ctx)
    }

    fn walk_mail_bodies<FN, E>(&self, use_it_fn: &mut FN) -> Result<(), E>
        where FN: FnMut(&Resource) -> Result<(), E>
    {
        use self::MailBody::*;
        match self.body {
            SingleBody { ref  body } =>
                use_it_fn(body)?,
            MultipleBodies { ref  bodies, .. } =>
                for body in bodies {
                    body.walk_mail_bodies(use_it_fn)?
                }
        }
        Ok(())
    }
}


impl MailBody {

    /// Returns `true` if it's an multipart body.
    pub fn is_multipart(&self) -> bool {
        use self::MailBody::*;
        match *self {
            SingleBody { .. } => false,
            MultipleBodies { .. } => true
        }
    }
}

/// A future resolving to an encodeable mail.
pub struct MailFuture<C: Context> {
    inner: InnerMailFuture<C>
}

enum InnerMailFuture<C: Context> {
    New { mail: Mail, ctx: C },
    Loading {
        mail: Mail,
        pending: future::JoinAll<Vec<ResourceLoadingFuture<C>>>,
        ctx: C
    },
    Poison
}

impl<C> MailFuture<C>
    where C: Context
{
    fn new(mail: Mail, ctx: C) -> Self {
        MailFuture { inner: InnerMailFuture::New { mail, ctx } }
    }
}

impl<T> Future for MailFuture<T>
    where T: Context,
{
    type Item = EncodableMail;
    type Error = MailError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        use self::InnerMailFuture::*;
        loop {
            let state = mem::replace(&mut self.inner, InnerMailFuture::Poison);
            match state {
                New { mail, ctx } => {
                    mail.generally_validate_mail()?;
                    top_level_validation(&mail)?;

                    let mut futures = Vec::new();
                    mail.walk_mail_bodies::<_, ()>(&mut |resource: &Resource| {
                        Ok(futures.push(resource.create_loading_future(ctx.clone())))
                    }).unwrap();

                    mem::replace(
                        &mut self.inner,
                        InnerMailFuture::Loading {
                            mail, ctx,
                            pending: future::join_all(futures)
                        }
                    );
                },
                Loading { mut mail, mut pending, ctx } => {
                    match pending.poll() {
                        Err(err) => return Err(err.into()),
                        Ok(Async::NotReady) => {
                            mem::replace(
                                &mut self.inner,
                                InnerMailFuture::Loading { mail, pending, ctx }
                            );
                            return Ok(Async::NotReady);
                        },
                        Ok(Async::Ready(guards)) => {
                            auto_gen_headers(&mut mail, &ctx);
                            return Ok(Async::Ready(EncodableMail(mail, guards)));
                        }
                    }
                },
                Poison => panic!("called again after completion (through value, error or panic)")
            }
        }
    }
}

/// a mail with all contained futures resolved, so that it can be encoded
//#[derive(Clone)]
pub struct EncodableMail(Mail, Vec<ResourceAccessGuard>);

impl EncodableMail {

    /// Encode the mail using the given encoding buffer.
    ///
    /// After encoding succeeded the buffer should contain
    /// a fully encoded mail including all attachments, embedded
    /// images alternate bodies etc.
    ///
    /// # Error
    ///
    /// This can fail for a large number of reasons, e.g. some
    /// input can not be encoded with the given mail type or
    /// some headers/resources breack the mails hard line length limit.
    pub fn encode(&self, encoder: &mut EncodingBuffer) -> Result<(), MailError> {
        ::encode::encode_mail(self, true, encoder)
    }

    /// A wrapper for `encode` which will create a buffer, enocde the mail and then returns the buffers content.
    pub fn encode_into_bytes(&self, mail_type: MailType) -> Result<Vec<u8>, MailError> {
        let mut buffer = EncodingBuffer::new(mail_type);
        self.encode(&mut buffer)?;
        Ok(buffer.into())
    }
}

fn top_level_validation(mail: &Mail) -> Result<(), HeaderValidationError> {
    if mail.headers().contains(_From) {
        Ok(())
    } else {
        Err(OtherValidationError::NoFrom.into())
    }
}

/// inserts ContentType and ContentTransferEncoding into
/// the headers of any contained `MailBody::SingleBody`,
/// based on the `Resource` representing the body
fn auto_gen_headers<C: Context>(mail: &mut Mail, ctx: &C) {
    {
        let headers = mail.headers_mut();
        if !headers.contains(Date) {
            headers.insert(Date::body(DateTime::now()));
        }

        if !headers.contains(MessageId) {
            headers.insert(MessageId::body(ctx.generate_message_id()));
        }
    }
    recursive_auto_gen_headers(mail, ctx);
}

fn recursive_auto_gen_headers<C: Context>(mail: &mut Mail, ctx: &C) {
    let &mut Mail { ref mut headers, ref mut body } = mail;
    match body {
        &mut MailBody::SingleBody { ref mut body } => {
            let file_buffer = body.get_if_encoded()
                .expect("[BUG] encoded mail, should only contain already transferencoded resources");
            headers.insert(ContentType::body(file_buffer.content_type().clone()));
            headers.insert(ContentTransferEncoding::body(file_buffer.transfer_encoding().clone()));

            if let Some(Ok(disposition)) = headers.get_single_mut(ContentDisposition) {
                let current_file_meta_mut = disposition.file_meta_mut();
                let resource = body.get_if_encoded()
                    .expect("[BUG] auto gen should only be called on all resources are loaded");
                current_file_meta_mut.replace_empty_fields_with(resource.file_meta())
            }
        },
        &mut MailBody::MultipleBodies { ref mut bodies, .. } => {
            let mut headers: &mut HeaderMap = headers;
            let content_type: &mut Header<ContentType> = headers
                .get_single_mut(ContentType)
                .expect("[BUG] mail was already validated")
                .expect("[BUG] mail was already validated");

            content_type.set_param(BOUNDARY, create_random_boundary());

            for sub_mail in bodies {
                recursive_auto_gen_headers(sub_mail, ctx);
            }
        }
    }
}

pub(crate) fn validate_multipart_headermap(headers: &HeaderMap)
    -> Result<(), MailError>
{

    if headers.contains(ContentTransferEncoding) {
        return Err(OtherValidationError::ContentTransferEncodingHeaderGiven.into());
    }
    if let Some(header) = headers.get_single(ContentType) {
        let header_with_right_type = header?;
        if !header_with_right_type.is_multipart() {
            return Err(OtherValidationError::SingleMultipartMixup.into());
        }
    } else {
        return Err(OtherValidationError::MissingContentTypeHeader.into());
    }
    headers.use_contextual_validators()?;
    Ok(())
}


pub(crate) fn validate_singlepart_headermap(headers: &HeaderMap)
    -> Result<(), HeaderValidationError>
{
    if headers.contains(ContentTransferEncoding) {
        return Err(OtherValidationError::ContentTransferEncodingHeaderGiven.into());
    }
    if headers.contains(ContentType) {
        return Err(OtherValidationError::ContentTypeHeaderGiven.into());
    }
    headers.use_contextual_validators()?;
    Ok(())
}

impl Deref for EncodableMail {

    type Target = Mail;
    fn deref( &self ) -> &Self::Target {
        &self.0
    }
}

impl Into<Mail> for EncodableMail {
    fn into(self) -> Mail {
        let EncodableMail(mut mail, _) = self;
        strip_encodable_mail(&mut mail);
        mail
    }
}

fn strip_encodable_mail(mail: &mut Mail) {
    let &mut Mail { ref mut headers, ref mut body } = mail;

    match body {
        &mut MailBody::SingleBody { .. } => {
            headers.remove(ContentType);
            headers.remove(ContentTransferEncoding);
        },
        &mut MailBody::MultipleBodies { ref mut bodies, .. } => {
            headers.remove(ContentTransferEncoding);
            for mail in bodies {
                strip_encodable_mail(mail)
            }
        }
    }
}

impl fmt::Debug for EncodableMail {
    fn fmt(&self, fter: &mut fmt::Formatter) -> fmt::Result {
        write!(fter, "EncodableMail {{ .. }}")
    }
}


#[cfg(test)]
mod test {
    use std::fmt::Debug;
    use headers::header_components::MediaType;
    use ::file_buffer::FileBuffer;
    use ::Resource;

    fn resource_from_text<I: Into<String>>(text: I) -> Resource {
        let text: String = text.into();
        let mt = MediaType::parse("text/plain; charset=utf-8").unwrap();
        let fb = FileBuffer::new(mt, text.into());
        Resource::sourceless_from_buffer(fb)
    }

    trait AssertDebug: Debug {}
    trait AssertSend: Send {}
    trait AssertSync: Sync {}

    mod Mail {
        #![allow(non_snake_case)]
        use std::str;
        use headers::{
            headers::{
                Subject,
                Comments
            }
        };
        use default_impl::test_context;
        use super::super::*;
        use super::resource_from_text;
        use super::{AssertDebug, AssertSend, AssertSync};

        fn load_blocking<C>(r: &Resource, ctx: &C) -> ResourceAccessGuard
            where C: Context
        {
            r.create_loading_future(ctx.clone()).wait().unwrap()
        }

        impl AssertDebug for Mail {}
        impl AssertSend for Mail {}
        impl AssertSync for Mail {}


        #[test]
        fn walk_mail_bodies_does_not_skip() {
            let mail = Mail {
                headers: HeaderMap::new(),
                body: MailBody::MultipleBodies {
                    bodies: vec! [
                        Mail {
                            headers: HeaderMap::new(),
                            body: MailBody::MultipleBodies {
                                bodies: vec! [
                                    Mail {
                                        headers: HeaderMap::new(),
                                        body: MailBody::SingleBody {
                                            body: resource_from_text("r1")
                                        }
                                    },
                                    Mail {
                                        headers: HeaderMap::new(),
                                        body: MailBody::SingleBody {
                                            body: resource_from_text("r2")
                                        }
                                    }
                                ],
                                hidden_text: Default::default()
                            }
                        },
                        Mail {
                            headers: HeaderMap::new(),
                            body: MailBody::SingleBody {
                                body: resource_from_text("r3")
                            }
                        }

                    ],
                    hidden_text: Default::default()
                }
            };

            let ctx = test_context();
            let mut body_count = 0;
            mail.walk_mail_bodies::<_, ()>(&mut |body: &Resource| {
                body_count += 1;
                let access = load_blocking(body, &ctx);
                let _encoded0 = access.access();
                let encoded = body.get_if_encoded().expect("it should be loaded");
                let slice = str::from_utf8(&encoded[..]).unwrap();
                assert!([ "r1", "r2", "r3"].contains(&slice));
                Ok(())
            }).unwrap();
        }

        #[test]
        fn walk_mail_bodies_handles_errors() {
            let mail = Mail {
                headers: HeaderMap::new(),
                body: MailBody::SingleBody {
                    body: resource_from_text("r0"),
                }
            };
            assert_ok!(mail.walk_mail_bodies::<_, ()>(&mut |_| { Ok(()) }));
            assert_err!(mail.walk_mail_bodies::<_, ()>(&mut |_| { Err(()) }));
        }

        test!(insert_header_set_a_header, {
            let mut mail = Mail::plain_text("r0");
            mail.insert_header(Subject::auto_body("hy")?);
            assert!(mail.headers().contains(Subject));
        });



        test!(insert_headers_sets_all_headers, {
            let mut mail = Mail::plain_text("r0");
            mail.insert_headers(headers! {
                Subject: "yes",
                Comments: "so much"
            }?);

            assert!(mail.headers().contains(Subject));
            assert!(mail.headers().contains(Comments));
        });

    }

    mod EncodableMail {
        #![allow(non_snake_case)]
        use chrono::{Utc, TimeZone};
        use headers::{
            header_components::TransferEncoding,
            headers::{
                _From, ContentType, ContentTransferEncoding,
                Date, Subject
            }
        };
        use default_impl::test_context;
        use super::super::*;
        use super::resource_from_text;
        use super::{AssertDebug, AssertSend, AssertSync};

        impl AssertDebug for EncodableMail {}
        impl AssertSend for EncodableMail {}
        impl AssertSync for EncodableMail {}

        #[test]
        fn sets_generated_headers_for_outer_mail() {
            let resource = resource_from_text("r9");
            let mail = Mail {
                headers: headers!{
                    _From: ["random@this.is.no.mail"],
                    Subject: "hoho"
                }.unwrap(),
                body: MailBody::SingleBody { body: resource }
            };

            let ctx = test_context();
            let enc_mail = assert_ok!(mail.into_encodeable_mail(ctx).wait());

            let headers: &HeaderMap = enc_mail.headers();
            assert!(headers.contains(_From));
            assert!(headers.contains(Subject));
            assert!(headers.contains(Date));
            assert!(headers.contains(ContentType));
            assert!(headers.contains(ContentTransferEncoding));
            assert!(headers.contains(MessageId));
            assert_eq!(headers.len(), 6);

            let res = headers.get_single(ContentType)
                .unwrap()
                .unwrap();

            assert_eq!(res.as_str_repr(), "text/plain; charset=utf-8");

            let res = headers.get_single(ContentTransferEncoding)
                .unwrap()
                .unwrap();

            assert_eq!(res.body(), &TransferEncoding::QuotedPrintable);
        }

        #[test]
        fn sets_generated_headers_for_sub_mails() {
            let resource = resource_from_text("r9");
            let mail = Mail {
                headers: headers!{
                    _From: ["random@this.is.no.mail"],
                    Subject: "hoho",
                    ContentType: "multipart/mixed"
                }.unwrap(),
                body: MailBody::MultipleBodies {
                    bodies: vec![
                        Mail {
                            headers: HeaderMap::new(),
                            body: MailBody::SingleBody { body: resource }
                        }
                    ],
                    hidden_text: Default::default()
                }
            };

            let ctx = test_context();
            let mail = mail.into_encodeable_mail(ctx).wait().unwrap();

            assert!(mail.headers().contains(_From));
            assert!(mail.headers().contains(Subject));
            assert!(mail.headers().contains(Date));
            //the Builder would have set it but as we didn't use it (intentionally) it's not set
            //assert!(headers.contains(ContentType));

            if let MailBody::MultipleBodies { ref bodies, ..} = mail.body {
                let headers = bodies[0].headers();
                assert_not!(headers.contains(Date));

                let res = headers.get_single(ContentType)
                    .unwrap()
                    .unwrap();

                assert_eq!(res.as_str_repr(), "text/plain; charset=utf-8");

                let res = headers.get_single(ContentTransferEncoding)
                    .unwrap()
                    .unwrap();

                assert_eq!(res.body(), &TransferEncoding::QuotedPrintable);

            } else {
                unreachable!()
            }
        }

        #[test]
        fn runs_contextual_validators() {
            let mail = Mail {
                headers: headers!{
                    _From: ["random@this.is.no.mail", "u.p.s@s.p.u"],
                    Subject: "hoho"
                }.unwrap(),
                body: MailBody::SingleBody { body: resource_from_text("r9") }
            };

            let ctx = test_context();
            assert_err!(mail.into_encodeable_mail(ctx).wait());
        }

        #[test]
        fn checks_there_is_from() {
            let mail = Mail {
                headers: headers!{
                    Subject: "hoho"
                }.unwrap(),
                body: MailBody::SingleBody { body: resource_from_text("r9") }
            };

            let ctx = test_context();
            assert_err!(mail.into_encodeable_mail(ctx).wait());
        }

        test!(does_not_override_date_if_set, {
            let provided_date = Utc.ymd(1992, 5, 25).and_hms(23, 41, 12);
            let mut mail = Mail::plain_text("r9");
            mail.insert_headers(headers! {
                _From: ["random@this.is.no.mail"],
                Subject: "hoho",
                Date: provided_date.clone()
            }?);

            let ctx = test_context();
            let enc_mail = assert_ok!(mail.into_encodeable_mail(ctx).wait());
            let used_date = enc_mail.headers()
                .get_single(Date)
                .unwrap()
                .unwrap();

            assert_eq!(&**used_date.body(), &provided_date);
        });

    }

}