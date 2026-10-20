# WorkOS privacy and exit assessment

Checked October 20, 2026. This review covers the assessed configuration: hosted AuthKit and Connect using the standard WorkOS login domain, with no custom login domain. It is based on public WorkOS materials and the accepted architecture in [ADR 0010](../adr/0010-external-identity-and-workspace-grants.md); the staging setup was inspected, but no order form, security report, or negotiated terms were inspected.

## Conclusion

The selected direction is managed WorkOS AuthKit and Connect without a custom
login domain. WorkOS owns users, organizations, memberships, roles, permissions,
and provider sessions. Pulse retains stable product IDs and analytics data. The
native Rust boundary, browser/MCP routes, and request-scoped RLS are fixture-tested.
A staging project is available; real browser/MCP proof and deployment remain
pending. WorkOS is the sole authority for managed membership and permissions.

A provider exit therefore includes authorization data and configuration, not just
user profiles. Export and reconcile organization membership, role definitions,
permission assignments, Connect applications, and mappings to Pulse IDs before
cutover. Public API access supports implementing such an export; this is not a
verified complete export tool or a promise that credentials and sessions move.
Plan fresh sign-in, password reset, and passkey re-enrollment where export is not
confirmed. Keep the WorkOS SDK and authorization resolution at the integration
boundary rather than limiting the product to the lowest common OIDC surface.

Using this configuration includes US processing. The DPA authorizes international
transfers and identifies US storage/access. The reviewed public materials do not
establish an EU-only deployment or regional pinning commitment. Record the
applicable DPA, transfer basis, processing locations, and service-specific terms
before collecting production identities; do not describe the default managed
configuration as EU-resident.

## Privacy and contract evidence

The [WorkOS Terms of Service](https://workos.com/legal/terms-of-service), updated August 4, 2026, govern unless a separate written agreement applies. An order form may add terms and controls over conflicting terms. The Terms incorporate the [Data Processing Addendum (DPA)](https://workos.com/legal/data-processing-addendum), whose public page is still dated June 27, 2023. Confirm that these are the versions and terms that will govern the AuthKit and Connect configuration when an account is created.

The DPA authorizes WorkOS and its subprocessors to transfer personal data across borders, expressly including from the EEA, Switzerland, and UK to the U.S. Its transfer questionnaire answers “United States of America” for the countries where data transferred out of those regions may be stored or accessed. This is evidence that U.S. processing is in scope; it does not prove that every record is stored only in the U.S., and it does not prove an EU-only option. The questionnaire says data is encrypted in transit. The DPA’s security exhibit also commits to encryption in transit and at rest “where appropriate,” rather than making a categorical at-rest encryption promise for every data class. The DPA also permits internal processing, where allowed by law, to improve service quality, detect security incidents, and prevent fraud or illegal activity; the Terms allow use of deidentified, aggregated usage and performance data. The DPA offers reasonable assistance with data-subject requests where required and limits its audit process to a questionnaire once per year. Confirm how these clauses apply to AuthKit events and telemetry, whether telemetry can be limited, and what support costs can apply to data-subject requests.

The current [WorkOS Trust Center](https://trust.workos.com/) is the source for its changing subprocessor and security materials. The public Trust Center summary lists AWS, Cloudflare, Datadog, and Postmark for Core Product in the U.S.; a February 27, 2026 [WorkOS update](https://trust.workos.com/) says ClickHouse was added for in-product reporting and insights. This is not a complete, AuthKit-specific inventory. The DPA gives WorkOS general authorization to use subprocessors, provides a 14-calendar-day objection window after notice, and places responsibility on the subscriber to check the list. Obtain the complete current list, roles, and processing locations for AuthKit, Connect, hosted UI, email, security/risk checks, logs, support, and backups; assign an owner to monitor changes.

The Terms prohibit using the service to collect or process “Sensitive Personal Data,” defined to include data classified as sensitive by law or policy and EU special-category data. Keep the WorkOS profile to the minimum authentication fields needed, such as email and display name. Keep reports, analytics data, workspace configuration, and report-recipient addresses in our service. WorkOS receives the organization
and access-management data needed for its managed authorization role. Do not put secrets or sensitive information in WorkOS metadata; WorkOS limits user metadata to ten key-value pairs and warns against storing sensitive information there.

At termination, the DPA says WorkOS will delete Subscriber Personal Data, except backups and archival copies deleted according to its retention schedule and data it must retain by law. The Terms separately say WorkOS may irrevocably delete account information, and that it will assist with exporting Subscriber Data for up to 30 days after termination only if the subscriber requested the assistance before termination. The Terms do not specify export format, object coverage, credential coverage, or an export fee. Request and verify the export before giving termination notice; obtain the backup-deletion schedule and written deletion confirmation process as contract inputs. WorkOS’s individual-user delete endpoint is documented as permanent and irreversible.

## Portability by data type

| Data or capability | Public evidence | Exit consequence |
| --- | --- | --- |
| User profile | The [User API](https://workos.com/docs/reference/authkit/user/list) supports paginated user listing, lookup, creation, update, and deletion. It accepts an `external_id` on create/update; WorkOS says it is unique per environment and at most 64 characters. | A bulk API export is feasible for profile records and IDs. No public one-click, complete WorkOS-to-another-provider export package was found. Use the contractual export assistance as a backstop, not as the only export plan. |
| Passwords | User creation/update and the [migration guide](https://workos.com/docs/migrate/other-services) accept imported password hashes. The public User API describes list/get responses as User objects but does not document returning password hashes. The [migration CLI](https://workos.com/docs/cli/migrations) documents moving users into WorkOS from other providers, not exporting users from WorkOS. | Do not assume password hashes are exportable. Ask WorkOS for a written answer covering hash algorithms, salts, MFA factors, and export conditions. Plan a password reset or re-enrollment for users unless export is confirmed and tested. |
| Sessions and tokens | AuthKit returns access and refresh tokens. The [session API](https://workos.com/docs/reference/authkit/session) can list active sessions and revoke a session. | No cross-provider session transfer is documented. Treat WorkOS sessions and refresh tokens as non-portable; cut over by issuing sessions from the new provider, requiring users to sign in again, then revoking remaining WorkOS sessions. This is an integration inference, not an explicit WorkOS guarantee. |
| Passkeys | [WorkOS passkeys](https://workos.com/docs/authkit/passkeys) are available only through hosted AuthKit UI. There is no self-service screen to view, rename, add, or remove them; operator removal is through the WorkOS Dashboard. WorkOS says passkeys are bound to the domain where registered and advises configuring the custom AuthKit domain before production enrollment; adding it later prevents use of credentials registered on the old domain. The [WebAuthn specification](https://www.w3.org/TR/webauthn-2/) scopes a credential key pair to a relying party and says the private key remains with the authenticator. | With the assessed standard WorkOS login domain, do not enable passkeys unless users can re-enroll when the login domain or provider changes. A provider exit should assume passkey re-enrollment. If passkeys become a requirement, revisit the domain decision before enrollment; WorkOS describes custom domains as a paid feature in its [custom domains guide](https://workos.com/docs/custom-domains). |
| WorkOS identities and links | [Identity Linking](https://workos.com/docs/authkit/identity-linking) says WorkOS links credentials using verified email ownership, and [Users and Organizations](https://workos.com/docs/authkit/users-organizations) says duplicate user emails are not allowed. | This is WorkOS’s account-linking behavior, not a safe rule for merging our local principals. Never attach local grants or merge local accounts solely because a WorkOS/new-provider email matches. |
| Product data and authorization | ADR 0010 keeps stable product IDs and analytics data in PostgreSQL; WorkOS owns users, organizations, memberships, roles, and permissions. | Export and validate both the product ID mapping and provider-managed grants. Preserving product IDs alone is insufficient for an authorization migration. |

## Three-year exit design

Keep app-owned principal and workspace UUIDs as references in analytics data and
audit. Bind each identity to an exact provider/environment and subject, and each
organization to its stable product workspace. Use WorkOS external IDs where
supported without copying analytics payloads or secrets into metadata. WorkOS
remains the source of membership and permission authority; a local mapping does
not authorize a user independently.

Before termination, export users and identity links, organizations, memberships,
role definitions and permission slugs, role assignments, relevant invitations and
policies, Connect client configuration/authorizations where exportable, and all
mappings to product UUIDs. Import or recreate equivalent authority in the new
provider, check allowed and denied access for each pilot case, and bind verified
new identities using controlled migration evidence. Verify that inactive users,
revoked memberships, and report-only recipients do not gain internal access.

Cut over only after the new browser and MCP login, tenant context/RLS, permission
changes, revocation, and rollback path pass. End old sessions and revoke remaining
old credentials. Request deletion after the export and grant reconciliation are
verified. Password hashes, MFA factors, passkeys, and sessions require separate
export answers; do not assume the API profile export includes them.

If a user cannot be linked by a carried-forward internal ID, use a controlled relink: while both providers are available, require an authenticated session for the existing local principal and a successful sign-in with the new provider, then have the user confirm the link. If the old provider is unavailable, use a documented account-recovery process with additional proof and operator review. A matching email address alone must never transfer a principal or its grants. This avoids depending on WorkOS’s automatic verified-email linking and handles changed, shared, recycled, or conflicting addresses.

Target an export/relink rehearsal by October 2028 and a provider decision or exit rehearsal by October 2029. The rehearsal should include users with password, social login, SSO, and passkey methods where enabled; organization memberships and role/permission assignments; email changes; duplicate/conflicting emails; session expiry and revocation; and rollback to the old issuer while the overlap window remains open.

## Inputs to resolve before production

1. Confirm the contracting entity, applicable ToS and order form, AuthKit/Connect service scope, fee schedule, renewal and termination notice, and whether a negotiated agreement changes standard terms.
2. Confirm the DPA applies to all AuthKit and Connect data in this deployment. Obtain the current signed/accepted DPA and the complete subprocessor list with the relevant service, data classes, and processing locations.
3. Ask WorkOS to confirm in writing whether AuthKit user records, authentication events, fraud/risk signals, support access, and backups can be pinned to the EEA. If not, record acceptance of U.S. processing; do not describe the deployment as EU-resident.
4. Get specific export answers for user profiles, verified status, linked social/SSO identities, password hashes, MFA factors, passkeys, Connect authorizations, session records, webhooks/events, custom configuration, and metadata. Confirm the export format, timing, support costs, and retention/deletion timeline, including backups.
5. Confirm the meaning and applicability of the “Sensitive Personal Data” prohibition for the authentication fields we plan to send. Confirm the service-improvement and telemetry scope, data-subject request costs, and the DPA’s questionnaire-based audit process. Obtain the current SOC 2 Type II report, security controls, incident notice, and support/continuity materials from the Trust Center for the vendor review record.
6. Keep passkeys disabled while using the standard WorkOS domain unless the product explicitly accepts re-enrollment at a future domain or provider change. Revisit this decision before enabling passkeys.

## Sources

All WorkOS sources below were checked October 20, 2026; legal pages may change. The Terms page states its August 4, 2026 update date; the DPA page states June 27, 2023.

- [WorkOS Terms of Service](https://workos.com/legal/terms-of-service)
- [WorkOS Data Processing Addendum](https://workos.com/legal/data-processing-addendum)
- [WorkOS Trust Center and subprocessors](https://trust.workos.com/)
- [WorkOS User API](https://workos.com/docs/reference/authkit/user/list)
- [WorkOS migration guide](https://workos.com/docs/migrate/other-services)
- [WorkOS migration CLI](https://workos.com/docs/cli/migrations)
- [WorkOS session guide](https://workos.com/docs/authkit/sessions)
- [WorkOS session API](https://workos.com/docs/reference/authkit/session)
- [WorkOS passkeys guide](https://workos.com/docs/authkit/passkeys)
- [WorkOS custom domains guide](https://workos.com/docs/custom-domains)
- [WorkOS identity-linking guide](https://workos.com/docs/authkit/identity-linking)
- [WorkOS users and organizations](https://workos.com/docs/authkit/users-organizations)
- [WorkOS metadata and external IDs](https://workos.com/docs/authkit/metadata)
- [W3C Web Authentication specification, Level 2](https://www.w3.org/TR/webauthn-2/)
