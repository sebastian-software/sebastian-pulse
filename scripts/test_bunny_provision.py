"""Protect production credential and target boundaries without contacting Bunny."""

import copy
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import urllib.error


SPEC = importlib.util.spec_from_file_location("pulse_provision", Path(__file__).with_name("provision-bunny.py"))
PROVISION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROVISION)


class ProvisioningTests(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads(PROVISION.MANIFEST.read_text())
        self.inputs = {
            "PULSE_IMAGE_TAG": "a" * 40,
            "PULSE_IMAGE_DIGEST": "sha256:" + "b" * 64,
            "DATABASE_URL": "postgresql://runtime:fixture-password@db.example.test/pulse",
            "WORKOS_API_KEY": "sk_live_fixture_only",
            "WORKOS_CLIENT_ID": "client_fixture_only",
            "WORKOS_AUTHKIT_URL": "https://pulse-fixture.authkit.app",
            "WORKOS_COOKIE_PASSWORD": "c" * 64,
        }

    def test_production_inputs_validate_without_mutating_the_template(self):
        original = copy.deepcopy(self.manifest)
        PROVISION.validate(self.manifest)
        PROVISION.validate(PROVISION.render(self.manifest, self.inputs), rendered=True)
        self.assertEqual(self.manifest, original)

    def test_recorded_release_is_usable_by_the_provisioning_template(self):
        release = json.loads((PROVISION.MANIFEST.parent / "release.workos.json").read_text())
        self.assertEqual(release["image"], "ghcr.io/sebastian-software/sebastian-pulse")
        self.assertEqual(release["platform"], "linux/amd64")
        self.inputs.update(PULSE_IMAGE_TAG=release["imageTag"], PULSE_IMAGE_DIGEST=release["imageDigest"])
        PROVISION.validate(PROVISION.render(self.manifest, self.inputs), rendered=True)

    def test_operator_and_migration_credentials_cannot_enter_runtime(self):
        for name in ("BUNNY_API_KEY", "DATABASE_MIGRATION_URL", "OIDC_CLIENT_SECRET"):
            with self.subTest(name=name):
                manifest = copy.deepcopy(self.manifest)
                manifest["containerTemplates"][0]["environmentVariables"].append({"name": name, "value": "fixture-secret"})
                with self.assertRaises(ValueError):
                    PROVISION.validate(manifest)

    def test_mutable_or_unpinned_images_fail_before_upload(self):
        for field, value in (("imageTag", "latest"), ("imageDigest", "sha256:short")):
            with self.subTest(field=field):
                rendered = PROVISION.render(self.manifest, self.inputs)
                rendered["containerTemplates"][0][field] = value
                with self.assertRaises(ValueError):
                    PROVISION.validate(rendered, rendered=True)

    def test_region_and_replica_changes_are_not_silently_deployed(self):
        self.manifest["regionSettings"]["allowedRegionIds"].append("NY")
        with self.assertRaises(ValueError):
            PROVISION.validate(self.manifest)
        self.setUp()
        self.manifest["autoScaling"]["max"] = 2
        with self.assertRaises(ValueError):
            PROVISION.validate(self.manifest)

    def test_staging_credentials_and_cross_origin_login_fail(self):
        for name, value in (("WORKOS_API_KEY", "sk_test_fixture"), ("WORKOS_AUTHKIT_URL", "https://pulse-fixture.authkit.app.attacker.test")):
            with self.subTest(name=name):
                inputs = {**self.inputs, name: value}
                with self.assertRaises(ValueError):
                    PROVISION.validate(PROVISION.render(self.manifest, inputs), rendered=True)

    def test_missing_input_does_not_echo_other_secret_values(self):
        del self.inputs["WORKOS_CLIENT_ID"]
        with self.assertRaises(ValueError) as caught:
            PROVISION.render(self.manifest, self.inputs)
        self.assertNotIn(self.inputs["WORKOS_API_KEY"], str(caught.exception))

    def test_invalid_connection_errors_do_not_echo_secret_values(self):
        self.inputs["DATABASE_URL"] = "postgresql://runtime:fixture-secret@db.test:not-a-port/pulse"
        with self.assertRaises(ValueError) as caught:
            PROVISION.validate(PROVISION.render(self.manifest, self.inputs), rendered=True)
        self.assertNotIn("fixture-secret", str(caught.exception))

    def test_duplicate_app_detection_follows_the_documented_cursor(self):
        with patch.object(PROVISION, "api", side_effect=[
            {"items": [{"name": "another-app"}], "cursor": "next page"},
            {"items": [{"name": "sebastian-pulse"}], "cursor": None},
        ]) as api:
            with self.assertRaisesRegex(ValueError, "already exists"):
                PROVISION.refuse_existing("fixture-key", "sebastian-pulse")
        self.assertEqual(api.call_count, 2)
        self.assertEqual(api.call_args.args[1], "/apps?limit=1000&nextCursor=next%20page")

    def test_provider_error_body_cannot_disclose_submitted_credentials(self):
        error = urllib.error.HTTPError("https://api.bunny.net/mc/apps", 422, "Unprocessable entity", {}, io.BytesIO(b'fixture-secret'))
        with patch.object(PROVISION.urllib.request, "build_opener") as build:
            build.return_value.open.side_effect = error
            with self.assertRaisesRegex(RuntimeError, "HTTP 422") as caught:
                PROVISION.api("POST", "/apps", "fixture-key", {"value": "fixture-secret"})
        self.assertNotIn("fixture-secret", str(caught.exception))
        self.assertEqual(error.fp.tell(), 0)

    def test_incomplete_inventory_cannot_allow_duplicate_creation(self):
        with patch.object(PROVISION, "api", return_value={"unexpected": []}):
            with self.assertRaisesRegex(RuntimeError, "inventory"):
                PROVISION.refuse_existing("fixture-key", "sebastian-pulse")


if __name__ == "__main__":
    unittest.main()
