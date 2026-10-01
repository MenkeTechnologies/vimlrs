" %S counts screen cells for both its width and its precision; %s counts bytes.
echo printf('%5s|%-5s|%5S|%.1S|%.2S|%.3S|%4.2S|', 'é', 'é', 'é', 'éa', '日本', '日本', '日本')
echo printf('%-6S|%6S|%.0S|', '日本', 'a日', 'x')
