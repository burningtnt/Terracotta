export async function main({context, octokit, require}) {
    const axios = require('axios') as typeof import('axios').default;
    const http = require('http') as typeof import('http');
    const https = require('https') as typeof import('https');

    axios.defaults.httpAgent = new http.Agent({ timeout: 60000 })
    axios.defaults.httpsAgent = new https.Agent({ timeout: 60000 })

    const pushUploadingJob = (function () {
        let jobs = new Map<string, { progress: number, estimated: number }>();
        let changed = false;

        setInterval(() => {
            if (!changed) {
                return;
            }

            changed = false;
            console.log(Array.from(jobs.entries()).sort((a, b) => a[0].localeCompare(b[0])).map(([name, {
                progress,
                estimated
            }]) => {
                let count = Math.floor(progress * 10);
                let p = `${name.padEnd(55, ' ')}  ${'\u{1F7E9}'.repeat(count)}${'\u{3000}'.repeat(10 - count)}  ${(progress * 100).toFixed(2)}%`;
                if (progress !== 1) {
                    p += ` ~${String(estimated).padStart(4, '0')}s left`
                }

                return p;
            }).join('\n') + "\n" + "=".repeat(10));
        }, 5000).unref();

        return (name: string, cfg: { progress: number, estimated: number }) => {
            changed = true;
            jobs.set(name, cfg);
        }
    }());

    let {
        tag_name: tagName,
        name,
        prerelease,
        body,
        assets: _assets
    } = (await octokit.rest.repos.getLatestRelease(context.repo)).data;

    let assets: { name: string, data: Blob }[] = await Promise.all(_assets.map(async (asset) => {
        const response = await fetch(asset.url, {
            "headers": {
                "Accept": "application/octet-stream"
            }
        });
        if (!response.ok) {
            throw new Error(`HTTP error: ${response.status}`);
        }

        if (!asset.name.endsWith("-pkg.tar.gz")) {
            return undefined;
        }

        return {name: asset.name, data: await response.blob()};
    })).then(l => l.filter(e => e));
    console.log(`Gathered ${assets.length} assets`);
    for (let asset of assets) {
        console.log(`- ${asset.name.padEnd(52, ' ')}: ${asset.data.size} bytes`);
    }

    await Promise.all([
        (async () => {
            const {id} = await fetch(`https://gitee.com/api/v5/repos/${process.env.GITEE_OWNER}/${process.env.GITEE_REPO}/releases`, {
                method: "POST",
                headers: {
                    "Content-Type": "application/x-www-form-urlencoded"
                },
                body: new URLSearchParams({
                    access_token: process.env.GITEE_TOKEN,
                    tag_name: tagName,
                    name: name,
                    body: body,
                    prerelease: prerelease,
                    target_commitish: process.env.GITEE_TARGET_COMMITISH
                }).toString()
            }).then(r => r.json());
            if (!id) {
                return;
            }

            return Promise.all(assets.map(async (asset) => {
                let form = new FormData();
                form.append("access_token", process.env.GITEE_TOKEN);
                form.append("file", asset.data, asset.name);

                await axios.postForm(`https://gitee.com/api/v5/repos/${process.env.GITEE_OWNER}/${process.env.GITEE_REPO}/releases/${id}/attach_files`, form, {
                    "onUploadProgress": ({progress, estimated}) => {
                        pushUploadingJob(`[GTE] ${asset.name}`, {progress, estimated});
                    }
                })
            }));
        })(),
        (async () => {
            const {id} = await axios.post(`https://api.cnb.cool/${process.env.CNB_OWNER}/${process.env.CNB_REPO}/-/releases`, {
                "body": body,
                "draft": false,
                "make_latest": name,
                "name": name,
                "prerelease": prerelease,
                "tag_name": tagName,
                "target_commitish": process.env.CNB_TARGET_COMMITISH
            }, {
                "headers": {
                    "Authorization": "Bearer " + process.env.CNB_TOKEN,
                    "Accept": "application/json",
                    "Content-Type": "application/json;charset=UTF-8"
                }
            }).then(r => r.data);
            if (!id) {
                return;
            }

            return Promise.all(assets.map(async (asset) => {
                const { upload_url: uploadURL, verify_url: verifyURL }: { upload_url: string, verify_url: string} = await axios.post(`https://api.cnb.cool/${process.env.CNB_OWNER}/${process.env.CNB_REPO}/-/releases/${id}/asset-upload-url`, {
                    "asset_name": asset.name,
                    "overwrite": true,
                    "size": asset.data.size
                }, {
                    "headers": {
                        "Authorization": "Bearer " + process.env.CNB_TOKEN,
                        "Accept": "application/json", // Fucking CNB API force Accept to be exact 'application/json'.
                        "Content-Type": "application/json;charset=UTF-8"
                    }
                }).then(r => r.data);

                let form = new FormData();
                form.append("file", asset.data, asset.name);
                await axios.postForm(uploadURL, form, {
                    "headers": {
                        "Authorization": "Bearer " + process.env.CNB_TOKEN,
                    },
                    "onUploadProgress": ({progress, estimated}) => {
                        pushUploadingJob(`[CNB] ${asset.name}`, {progress, estimated});
                    }
                });

                await axios.post(verifyURL, {}, {
                    "headers": {
                        "Authorization": "Bearer " + process.env.CNB_TOKEN,
                        "Accept": "application/json", // Fucking CNB API force Accept to be exact 'application/json'.
                    }
                })
            }));
        })()
    ]);
}
