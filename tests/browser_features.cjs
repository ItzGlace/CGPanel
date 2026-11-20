// Run through tests/browser_features.py. Requires Playwright and a Chromium binary.
const {chromium}=require('playwright');const assert=require('node:assert/strict');const fs=require('node:fs');
(async()=>{const browser=await chromium.launch({executablePath:process.env.CGPANEL_CHROMIUM||(fs.existsSync('/usr/bin/chromium')?'/usr/bin/chromium':undefined),headless:true,args:['--no-sandbox']});
const page=await browser.newPage({viewport:{width:1440,height:1000}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
await page.goto(process.env.CGPANEL_BROWSER_URL);await page.locator('[name=username]').fill('admin');await page.locator('[name=password]').fill(process.env.CGPANEL_BROWSER_PASSWORD);await page.locator('#login button[type=submit]').click();await page.locator('#panel').waitFor({state:'visible'});await page.evaluate(()=>{for(const key of Object.keys(localStorage))localStorage.removeItem(key);document.querySelector('#cgp-tour')?.remove();localStorage.setItem('cgp-tour-v5-'+JSON.parse('null'),'1')});
for(const width of [1440,390]){await page.setViewportSize({width,height:1000});for(const route of ['protection','captcha','analytics','mcp','login-security','database-studio','domains','apps','mail','updates']){
await page.goto(process.env.CGPANEL_BROWSER_URL+'/#'+route);await page.waitForFunction(()=>!document.querySelector('#content .busy'));await page.waitForTimeout(100);await page.evaluate(()=>document.querySelector('#cgp-tour')?.remove());
const text=await page.locator('#content').innerText();assert(!text.includes('Failed to')&&!text.includes('Unknown operation'),route+': '+text);
if(['protection','captcha'].includes(route)){
 const field=page.locator('#protection-form input').first();assert(await field.evaluate(el=>getComputedStyle(el).display)==='block'||await field.evaluate(el=>el.getBoundingClientRect().width)>150);
 assert(await page.locator('#protection-form').evaluate(el=>el.getBoundingClientRect().right<=innerWidth+1),route+' form overflows');
 if(route==='captcha')assert.equal(await page.locator('[name=secret]').getAttribute('required'),null);
}
if(route==='analytics')await page.locator('#heatmap-kind').selectOption('move');
if(route==='domains')assert(text.includes('Subdomain of example.com'));
fs.mkdirSync(process.env.CGPANEL_SCREENSHOTS,{recursive:true});await page.screenshot({path:process.env.CGPANEL_SCREENSHOTS+'/'+route+'-'+width+'.png',fullPage:true});
}}
await page.setViewportSize({width:1440,height:1000});await page.goto(process.env.CGPANEL_BROWSER_URL+'/#editor?app='+('c'.repeat(32))+'&path=test.txt');await page.locator('#fm-text').waitFor();await page.locator('#fm-find').fill('NEEDLE');await page.locator('#fm-find').press('Enter');assert((await page.locator('#fm-text').evaluate(el=>el.scrollTop))>500);assert.equal(await page.locator('#fm-text').evaluate(el=>el.value.slice(el.selectionStart,el.selectionEnd)),'NEEDLE');await page.locator('#fm-find').press('Enter');assert((await page.locator('#fm-find-status').innerText()).includes('Wrapped'));await page.locator('#fm-text').press('Control+f');assert(await page.locator('#fm-find').evaluate(el=>el===document.activeElement));
assert.deepEqual(errors,[]);await browser.close();console.log('PASS: 20 desktop/mobile views, analytics controls, domain grouping, editor find/scroll/wrap/Ctrl+F; no browser exceptions');})().catch(e=>{console.error(e);process.exit(1)});
