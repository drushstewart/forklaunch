import {createForkLaunchSdk,type VersionsVersionsWriteVersionV1Request,type VersionsVersionsWriteVersionV2Request} from './index.js';
export async function verify(baseUrl:string) {
 const sdk=createForkLaunchSdk({baseUrl});
 const v1:VersionsVersionsWriteVersionV1Request={body:{kind:'v1',value:'水 🦀'}};
 const v2:VersionsVersionsWriteVersionV2Request={body:{kind:'v2',value:42}};
 // @ts-expect-error Version 1 retains its string body.
 const bad1:VersionsVersionsWriteVersionV1Request={body:{kind:'v1',value:42}};
 // @ts-expect-error Version 2 retains its discriminant and integer body.
 const bad2:VersionsVersionsWriteVersionV2Request={body:{kind:'v1',value:'bad'}};
 void bad1;void bad2;
 const one=await sdk.versions.versions.writeVersionV1(v1);
 const two=await sdk.versions.versions.writeVersionV2(v2);
 if(one.code!==201 || one.response.value!=='水 🦀')throw Error('Version 1 response mismatch');
 if(two.code!==202 || two.response.value!==42)throw Error('Version 2 response mismatch');
 const transformed=await sdk.versions.versionedTransform.writeTransformVersionV2({body:{value:' ab '}});
 if(transformed.code!==202 || transformed.response.value!=='ab!')throw Error('Transformed response mismatch');
 const fallback=await sdk.versions.versionedTransform.writeTransformVersionV1({body:{value:'blocked'}});
 if(fallback.code!==201 || fallback.response.value!=='BLOCKED')throw Error('Transform fallback mismatch');
 console.log('PASS typed TypeScript versioned SDK');
}
