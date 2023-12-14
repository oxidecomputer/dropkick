
## Things you might want to update as general maintenance:

- `flake.nix` inputs
    - re-run `nix flake update / nix flake lock`
- caddy/DynamoDB
    - See `src/nix/caddy/default.nix` for instructions
- dropkick rust dependencies

## Testing Changes

We'll be testing with the `beachball` project in the `examples/` repo of this
project.

Install dropkick from your local repo:

```bash
cargo install --locked --path $DROPKICK_REPO
```

### If you need to recreate the CDK stack

Built/upload the ec2 image for the example project. You don't need to specify port because that's in the example project's cargo toml:

**bash**:

```bash
export BEACHBALL_AMI_ID="$(AWS_DEFAULT_REGION=us-east-2 dropkick create-ec2-image --hostname 'ball.iliana.0xeng.dev' --cert-storage dynamodb $DROPKICK_REPO/examples/beachball | tail -n1)"
```

or **fish**:

```fish
set -x BEACHBALL_AMI_ID (AWS_DEFAULT_REGION=us-east-2 dropkick create-ec2-image --hostname 'ball.iliana.0xeng.dev' --cert-storage dynamodb $DROPKICK_REPO/examples/beachball | tail -n1)
```

If you get one of these errors:

```
Error: failed to construct request

Caused by:
    no credentials in the property bag
```

or

```
Error: service error

Caused by:
    0: unhandled error
    1: unhandled error
    2: Error { code: "RequestExpired", message: "Request has expired." }
```

Your AWS creds don't exist or are out of date and you need to log in again.

Anyways, once `dropkick create-ec2-image` works, `BEACHBALL_AMI_ID` should be set to the AMI ID of beachball.

Check

```bash
echo $BEACHBALL_AMI_ID
```

And you should get something like `ami-07be9342bbbbbbbbb`

Move into the cdk directory for the beachball test project:

```bash
cd $DROPKICK_REPO/examples/beachball/cdk
```

Deploy the cloud formation:

```bash
AWS_DEFAULT_REGION=us-east-2 npm run cdk deploy -- --parameters DropkickImageId=$BEACHBALL_AMI_ID
```

The output will print two IP addresses, like this:

```
BeachballStack.DropkickServicePublicIpv4 = whatever
BeachballStack.DropkickServicePublicIpv6 = whatever
```

If these do not match the DNS entries for `ball.iliana.0xeng.dev`, you need to update DNS.

After all that though, you can finally check that it works

```
curl -4 https://ball.iliana.0xeng.dev/; echo
curl -6 https://ball.iliana.0xeng.dev/; echo
```

It'll print out some sample JSON data, as well as as the currently running ami
ID, so you can be sure that the changes you deployed actually exist:

```
{"ship":"yes","color":"grey","ami_id":"ami-0d8faa4b8b17d4faf"}
```


### If you don't need to recreate the CDK stack

```bash
AWS_DEFAULT_REGION=us-east-2 dropkick deploy-ec2-image --hostname 'ball.iliana.0xeng.dev' --cert-storage dynamodb $DROPKICK_REPO/examples/beachball BeachballStack
```

If you get one of these errors:

```
Error: failed to construct request

Caused by:
    no credentials in the property bag
```

or

```
Error: service error

Caused by:
    0: unhandled error
    1: unhandled error
    2: Error { code: "RequestExpired", message: "Request has expired." }
```

Your AWS creds don't exist or are out of date and you need to log in again.

Anyways, if this works, the new image should be created and deployed, and you can test it like before:

```
curl -4 https://ball.iliana.0xeng.dev/; echo
curl -6 https://ball.iliana.0xeng.dev/; echo
```

It'll print out some sample JSON data, as well as as the currently running ami
ID, so you can be sure that the changes you deployed actually exist:

```
{"ship":"yes","color":"grey","ami_id":"ami-0d8faa4b8b17d4faf"}
```
