"""Launch an allowlisted Semwright iteration using a runner-injected secret.

No token is read from Git/browser/local storage, returned, or written to artifacts.
POST is attempted once; ambiguous network outcomes require inspection, not retry.
"""
import json
import os
from pathlib import Path
import re
import time
import urllib.error
import urllib.request

REPO = "seradotcom/semwright"
PROJECT = "d6429a9c-77ef-4dca-9adc-7042536a4cf6"
SLUG = "circleci/2iTFoXJYHZ4dhHfXZwFsii/TTYuMs1YV3UxUvgg1y6v9j"
PRODUCT = "cd518748f742025a251b78028613aa1b16919e73"
TARGETS = {
    "h-harness": {"branch":"ci/h-circleci-iteration-9c7b3fb", "sha":"c2a6689493b88945835b2a4ce148124b339be322"},
    "portable": {"branch":"ci/i-circleci-iteration-cd51874", "sha":"21c06da1a1687c697c6126ede18e940b0087284d"},
}
LANES = ("smoke","composition","graph-effects","authoring","all","retest-affected")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(url, *, token, body=None):
    headers = {"Accept":"application/json"}
    if url.startswith("https://circleci.com/api/v2/"):
        headers["Circle-Token"] = token
    elif not url.startswith("https://api.github.com/repos/"+REPO+"/"):
        raise RuntimeError("Unapproved API destination")
    data = None
    if body is not None:
        headers["Content-Type"] = "application/json"
        data = json.dumps(body).encode()
    req = urllib.request.Request(url, headers=headers, data=data)
    opener = urllib.request.build_opener(NoRedirect())
    try:
        with opener.open(req,timeout=25) as response:
            raw = response.read(1_048_577)
            if len(raw)>1_048_576:
                raise RuntimeError("Oversized API response")
            return json.loads(raw)
    except urllib.error.HTTPError as error:
        # Error bodies can contain service internals. Do not persist or print them.
        raise RuntimeError("API request failed: HTTP %d"%error.code) from None
    except (urllib.error.URLError, TimeoutError):
        raise RuntimeError("API transport failed; do not retry a possibly accepted launch") from None


def main():
    if os.environ.get("GITHUB_ACTIONS") != "true" or os.environ.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise RuntimeError("GitHub-hosted bridge only")
    token = os.environ.get("CIRCLE_TOKEN")
    if not token:
        raise RuntimeError("CIRCLE_TOKEN repository secret unavailable")
    target_name = os.environ["SW_CIRCLE_TARGET"]
    if target_name not in TARGETS:
        raise RuntimeError("Unapproved CircleCI target")
    target = TARGETS[target_name]
    if target_name == "h-harness":
        target = {"branch":os.environ["SW_H_CIRCLE_BRANCH"],"sha":os.environ["SW_H_CIRCLE_SUITE"]}
        if (not re.fullmatch(r"ci/h-circleci-iteration-[0-9a-f]{7}",target["branch"])
                or not re.fullmatch(r"[0-9a-f]{40}",target["sha"])):
            raise RuntimeError("Invalid reviewed H iteration branch/immutable suite identity")
    lane = os.environ["SW_CIRCLE_LANE"]
    if lane not in LANES:
        raise RuntimeError("Unapproved diagnostic lane")
    out = Path("verification/circleci-launcher")
    out.mkdir(parents=True,exist_ok=False)
    record = {"schema_version":1,"bridge_suite_sha":os.environ["GITHUB_SHA"],
              "bridge_run_id":os.environ["GITHUB_RUN_ID"], "project_id":PROJECT,
              "project_slug":SLUG,"target":target_name,"branch":target["branch"],
              "expected_circle_suite_sha":target["sha"],"technical_product_target_sha":PRODUCT,
              "parameters":{} if target_name=="h-harness" else {"lane":lane,"candidate-sha":PRODUCT},
              "state":"AUTHENTICATION_AND_IDENTITY_CHECK_PENDING","post_attempts":0,
              "certification_eligible":False,"model_evaluation_executed":False,"r16_closed":False}
    def save():
        (out/"launch.json").write_text(json.dumps(record,indent=2)+"\n")
    save()
    try:
        ref = request("https://api.github.com/repos/"+REPO+"/git/ref/heads/"+target["branch"],token=token)
        if ref["object"]["sha"] != target["sha"]:
            raise RuntimeError("Target branch moved; review identity before launching")
        definitions = request("https://circleci.com/api/v2/projects/"+PROJECT+"/pipeline-definitions",token=token)
        candidates=[]
        public=[]
        for item in definitions.get("items",[]):
            public.append({"id":item["id"],"name":item["name"],
                           "config_file":item.get("config_source",{}).get("file_path"),
                           "config_repo":item.get("config_source",{}).get("repo",{}).get("full_name"),
                           "checkout_repo":item.get("checkout_source",{}).get("repo",{}).get("full_name")})
            config=item.get("config_source",{})
            checkout=item.get("checkout_source",{})
            if (config.get("repo",{}).get("full_name")==REPO and
                    checkout.get("repo",{}).get("full_name")==REPO and config.get("file_path")==".circleci/config.yml"):
                candidates.append(item)
        record["authenticated_definition_lookup"]=True
        record["pipeline_definitions"]=public
        preferred=[item for item in candidates if item["name"]=="A Composition Iteration"]
        if len(preferred)==1:
            candidates=preferred
        if len(candidates)!=1:
            raise RuntimeError("No unique Semwright config/checkout definition; inspect recorded definition IDs")
        selected=candidates[0]
        body={"definition_id":selected["id"],"config":{"branch":target["branch"]},
              "checkout":{"branch":target["branch"]},"parameters":record["parameters"]}
        record.update(definition_id=selected["id"],definition_name=selected["name"],
                      state="LAUNCH_ATTEMPT_RECORDED",post_attempts=1)
        save()
        pipeline=request("https://circleci.com/api/v2/project/"+SLUG+"/pipeline/run",token=token,body=body)
        if not re.fullmatch(r"[0-9a-f-]{36}",pipeline.get("id","")) or type(pipeline.get("number")) is not int:
            raise RuntimeError("Launch response missing pipeline identity; inspect before retry")
        record["pipeline"]={k:pipeline[k] for k in ("id","number","state","created_at") if k in pipeline}
        record["state"]="PIPELINE_ACCEPTED_WORKFLOWS_PENDING"
        save()
        # Only observe here. Compilation/native work is entirely on CircleCI.
        deadline=time.monotonic()+90
        while time.monotonic()<deadline:
            data=request("https://circleci.com/api/v2/pipeline/"+pipeline["id"]+"/workflow",token=token)
            workflows=[{k:item[k] for k in ("id","name","status","created_at","stopped_at") if k in item} for item in data.get("items",[])]
            record["workflows"]=workflows
            if workflows:
                record["state"]="PIPELINE_LAUNCHED_WORKFLOWS_OBSERVED"
                save()
                break
            time.sleep(5)
        print("CircleCI pipeline",pipeline["number"],pipeline["id"],record["state"])
    except Exception as error:
        record["state"]="BLOCKED_OR_AMBIGUOUS_LAUNCH_REQUIRES_INSPECTION"
        record["failure"]={"type":type(error).__name__,"message":str(error)}
        save()
        raise


if __name__=="__main__":
    main()
