// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

import type {
  CloudFormationCustomResourceHandler,
  CloudFormationCustomResourceResponse,
} from "aws-lambda";

export const handler: CloudFormationCustomResourceHandler = async (event) => {
  const success =
    event.RequestType === "Delete"
      ? true
      : await Promise.race([
          ping(event.ResourceProperties.PublicIp),
          timeout(),
        ]);
  const response: CloudFormationCustomResourceResponse = {
    ...(success
      ? {
          Status: "SUCCESS",
        }
      : {
          Status: "FAILED",
          Reason: "Timed out after 10 minutes",
        }),
    PhysicalResourceId:
      event.RequestType === "Create" ? "HealthCheck" : event.PhysicalResourceId,
    StackId: event.StackId,
    RequestId: event.RequestId,
    LogicalResourceId: event.LogicalResourceId,
  };
  await fetch(event.ResponseURL, {
    method: "PUT",
    body: JSON.stringify(response),
  });
};

async function ping(ip: string): Promise<true> {
  for (;;) {
    const delay = new Promise((resolve) => setTimeout(resolve, 1000));
    try {
      const response = await fetch(`http://${ip}/ping`, {
        redirect: "manual",
        signal: AbortSignal.timeout(1000),
      });
      if (response.status < 400) {
        return true;
      }
    } catch (err) {
      // do nothing
    }
    await delay;
  }
}

async function timeout(): Promise<false> {
  return new Promise((resolve) =>
    setTimeout(() => resolve(false), 10 * 60 * 1000),
  );
}
